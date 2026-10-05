package proxy

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log/slog"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestAuthenticationWarmupRunsIndependentStagesTogether(t *testing.T) {
	started := make(chan struct{}, 2)
	release := make(chan struct{})
	warm := func(ctx context.Context) error {
		started <- struct{}{}
		select {
		case <-release:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	warmup := startAuthenticationWarmup(t.Context(), slog.New(slog.DiscardHandler), warm, warm)
	defer warmup.stop()
	<-started
	<-started
	close(release)
}

func TestAuthenticationWarmupShutdownCancelsAndJoinsBothStages(t *testing.T) {
	started := make(chan struct{}, 2)
	finished := make(chan struct{}, 2)
	warm := func(ctx context.Context) error {
		started <- struct{}{}
		<-ctx.Done()
		finished <- struct{}{}
		return ctx.Err()
	}
	warmup := startAuthenticationWarmup(t.Context(), slog.New(slog.DiscardHandler), warm, warm)
	<-started
	<-started
	warmup.stop()
	if len(finished) != 2 {
		t.Fatal("shutdown did not finish both authentication warmup stages")
	}
}

func TestAuthenticationOnlyNetworkNeverOpensTransport(t *testing.T) {
	network := authenticationOnlyNetwork{}
	if conn, err := network.DialContext(t.Context(), "not-a-network-address"); conn != nil || !errors.Is(err, errAuthenticationPrepared) {
		t.Fatal("authentication-only network opened a transport")
	}
	if data, err := network.PingContext(t.Context(), "not-a-network-address"); data != nil || !errors.Is(err, errAuthenticationPrepared) {
		t.Fatal("authentication-only network sent a ping")
	}
	if listener, err := network.Listen("not-a-network-address"); listener != nil || !errors.Is(err, errAuthenticationPrepared) {
		t.Fatal("authentication-only network opened a listener")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := network.DialContext(ctx, ""); !errors.Is(err, context.Canceled) {
		t.Fatal("authentication-only network ignored cancellation")
	}
}

func TestAuthenticationWarmupContainsAndRedactsFailures(t *testing.T) {
	var output bytes.Buffer
	warmup := startAuthenticationWarmup(t.Context(), slog.New(slog.NewJSONHandler(&output, nil)), func(context.Context) error {
		panic("private authentication panic")
	}, func(context.Context) error {
		return errors.New("private authentication credential")
	})
	<-warmup.done
	warmup.stop()
	if !warmup.failed.Load() || strings.Contains(output.String(), "private") {
		t.Fatal("warmup lost a failure or exposed authentication details")
	}
}

type countingTokenSource struct{ calls atomic.Int32 }

func (source *countingTokenSource) Token() (*oauth2.Token, error) {
	source.calls.Add(1)
	return &oauth2.Token{AccessToken: "fixture", Expiry: time.Now().Add(time.Hour)}, nil
}

func TestAuthenticationWarmupNeverRunsSignedOut(t *testing.T) {
	warmUpstreamAuthentication(t.Context(), nil, slog.New(slog.DiscardHandler)).stop()
	oauth := new(countingTokenSource)
	account := authcache.NewAccount(t.Context(), "", oauth, io.Discard)
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	before := oauth.calls.Load()
	warmup := warmUpstreamAuthentication(t.Context(), account, slog.New(slog.DiscardHandler))
	select {
	case <-warmup.done:
	default:
		t.Fatal("signed-out warmup started work")
	}
	warmup.stop()
	if oauth.calls.Load() != before || warmup.failed.Load() {
		t.Fatal("signed-out warmup touched the account")
	}
}
