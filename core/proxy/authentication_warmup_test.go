package proxy

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestAuthenticationKeepWarmNeverRunsSignedOut(t *testing.T) {
	keepAuthenticationWarm(t.Context(), nil, slog.New(slog.DiscardHandler))()
	account := authcache.NewAccount(t.Context(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "fixture", Expiry: time.Now().Add(time.Hour)}), io.Discard)
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	keepAuthenticationWarm(t.Context(), account, slog.New(slog.DiscardHandler))()
}

func TestAuthenticationKeepWarmStopCancelsAndJoinsBothWorkers(t *testing.T) {
	started := make(chan struct{}, 2)
	finished := make(chan struct{}, 2)
	stop := startAuthenticationKeepWarm(t.Context(), slog.New(slog.DiscardHandler), func(ctx context.Context) error {
		started <- struct{}{}
		<-ctx.Done()
		finished <- struct{}{}
		return ctx.Err()
	}, func(ctx context.Context) {
		started <- struct{}{}
		<-ctx.Done()
		finished <- struct{}{}
	})
	<-started
	<-started
	stop()
	if len(finished) != 2 {
		t.Fatal("stop returned before both authentication workers finished")
	}
}

func TestAuthenticationKeepWarmContainsAndRedactsFailures(t *testing.T) {
	var output bytes.Buffer
	stop := startAuthenticationKeepWarm(t.Context(), slog.New(slog.NewJSONHandler(&output, nil)), func(context.Context) error {
		return errors.New("private authentication detail")
	}, func(context.Context) {
		panic("private authentication panic")
	})
	stop()
	if !strings.Contains(output.String(), `"success":false`) || strings.Contains(output.String(), "private") {
		t.Fatalf("preload failure was lost or exposed details: %s", output.String())
	}
}
