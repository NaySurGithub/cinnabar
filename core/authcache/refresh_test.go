package authcache

import (
	"context"
	"encoding/json"
	"path/filepath"
	"regexp"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

func countingServiceDeps(exchanges *atomic.Int32, deviceIDs chan<- string) derivedDeps {
	return derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, token *service.Token, deviceID string) service.TokenSource {
			if deviceIDs != nil {
				deviceIDs <- deviceID
			}
			return fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
				exchanges.Add(1)
				return testServiceToken(time.Now().Add(time.Hour)), nil
			})(env, tickets, token, deviceID)
		},
	}
}

// rewriteXUID replaces the fixture account's XUID in its persisted bundle.
func rewriteXUID(t *testing.T, path, xuid string) {
	t.Helper()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, token := range state.SISU.XSTSTokens {
		for index := range token.DisplayClaims.UserInfo {
			token.DisplayClaims.UserInfo[index].XUID = xuid
		}
	}
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
}

func serviceDeviceID(t *testing.T, xuid string) string {
	t.Helper()
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	rewriteXUID(t, path, xuid)
	var exchanges atomic.Int32
	deviceIDs := make(chan string, 4)
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, countingServiceDeps(&exchanges, deviceIDs))
	defer account.Close()
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	return <-deviceIDs
}

// Service sessions present one device per account across processes, never a fresh random one.
func TestServiceDeviceIDIsStablePerAccount(t *testing.T) {
	first, again, other := serviceDeviceID(t, "123"), serviceDeviceID(t, "123"), serviceDeviceID(t, "456")
	if !regexp.MustCompile(`^[0-9a-f]{32}$`).MatchString(first) || first != again || first == other {
		t.Fatalf("device IDs = %q, %q, %q; want one undashed ID per account", first, again, other)
	}
	if unknown := serviceDeviceID(t, ""); unknown != "" {
		t.Fatalf("device ID without an XUID = %q, want the random fallback", unknown)
	}
}

// A service token near expiry is replaced in the background and persisted for later processes.
func TestRefreshAheadReplacesServiceTokenBeforeExpiry(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(serviceRefreshLead/2))
	var exchanges atomic.Int32
	deps := countingServiceDeps(&exchanges, nil)
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	expiry, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead)
	if err != nil || exchanges.Load() != 1 || time.Until(expiry) <= serviceRefreshLead {
		t.Fatalf("refresh: err=%v exchanges=%d expiry_in=%v", err, exchanges.Load(), time.Until(expiry))
	}
	if _, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead); err != nil || exchanges.Load() != 1 {
		t.Fatalf("fresh token was refreshed again: err=%v exchanges=%d", err, exchanges.Load())
	}
	other := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer other.Close()
	if _, err := other.refreshServiceAhead(context.Background(), serviceRefreshLead); err != nil || exchanges.Load() != 1 {
		t.Fatalf("another process refreshed a persisted fresh token: err=%v exchanges=%d", err, exchanges.Load())
	}
}

// Signing out ends the background refresher instead of leaving it waiting for the next expiry.
func TestKeepFreshStopsWhenAccountCloses(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var exchanges atomic.Int32
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, countingServiceDeps(&exchanges, nil))
	done := make(chan struct{})
	go func() {
		defer close(done)
		account.KeepFresh(context.Background())
	}()
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("KeepFresh outlived the account")
	}
	if exchanges.Load() != 0 {
		t.Fatalf("a fresh token was exchanged %d times", exchanges.Load())
	}
}
