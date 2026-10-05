package authcache

import (
	"context"
	"encoding/hex"
	"errors"
	"io"
	"time"

	"github.com/google/uuid"
	"golang.org/x/oauth2"
)

const (
	serviceRefreshLead = 10 * time.Minute // replace the service token this long before it expires
	refreshAttempt     = 30 * time.Second // bounds one background exchange so a hung request cannot wedge refresh
	refreshRecheck     = 15 * time.Minute // longest sleep, so suspend or another process's refresh is noticed
	refreshRetryMin    = time.Minute
	refreshRetryMax    = 15 * time.Minute
)

// serviceDeviceNamespace scopes the per-account Minecraft service device ID.
var serviceDeviceNamespace = uuid.MustParse("52194710-c463-4fa4-98a9-334c1b905e01")

// CompleteSignIn exchanges the signed-in account's service token and persists it next to the Microsoft
// token at oauthPath, so a join only mints its key-bound token.
func CompleteSignIn(ctx context.Context, oauthPath string, oauth oauth2.TokenSource, diagnostics io.Writer) error {
	return completeSignIn(ctx, oauthPath, oauth, diagnostics, defaultDerivedDeps())
}

func completeSignIn(ctx context.Context, oauthPath string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) error {
	account := newAccount(ctx, DerivedCachePath(oauthPath), oauth, diagnostics, deps)
	if account == nil {
		return errors.New("authentication: no signed-in account")
	}
	defer func() { _ = account.Close() }()
	_, err := account.ServiceToken(ctx)
	return err
}

// KeepFresh refreshes the cached service token shortly before it expires while signed in. It returns
// when ctx ends or the account closes, and at once when another KeepFresh already serves this account.
func (s *Account) KeepFresh(ctx context.Context) {
	if !s.refreshing.CompareAndSwap(false, true) {
		return
	}
	defer s.refreshing.Store(false)
	retry := refreshRetryMin
	for {
		attempt, cancel := context.WithTimeout(ctx, refreshAttempt)
		remaining, err := s.refreshServiceAhead(attempt, serviceRefreshLead)
		cancel()
		var wait time.Duration
		switch {
		case ctx.Err() != nil || s.Closed() || errors.Is(err, ErrAccountClosed) || errors.Is(err, errAccountChanged):
			return
		case err != nil:
			wait, retry = retry, min(retry*2, refreshRetryMax)
		default:
			wait, retry = remaining-serviceRefreshLead, refreshRetryMin
		}
		timer := time.NewTimer(min(max(wait, refreshRetryMin), refreshRecheck))
		select {
		case <-ctx.Done():
		case <-s.ctx.Done():
		case <-timer.C:
			continue
		}
		timer.Stop()
		return
	}
}

// refreshServiceAhead replaces the service token once it is within lead of expiry and returns how long
// the current token remains valid, on the service clock its validity uses. A failed early exchange keeps
// the still-valid token; a token another process already refreshed is reused through the shared cache.
func (s *Account) refreshServiceAhead(ctx context.Context, lead time.Duration) (time.Duration, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return 0, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return 0, err
	}
	lease, err := s.acquireLeaseLocked(ctx)
	if err != nil {
		return 0, err
	}
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	if s.service != nil && s.service.Valid() && s.service.Remaining() > lead {
		return s.service.Remaining(), nil
	}
	token, err := s.serviceTokenLocked(ctx, lease != nil, true)
	if err != nil {
		return 0, err
	}
	return token.Remaining(), nil
}

// serviceDeviceIDLocked returns the account's stable, undashed service device ID derived from its XUID,
// as an install keeps one device ID across restarts; "" before any token carries the XUID.
func (s *Account) serviceDeviceIDLocked() string {
	snapshot := s.session.Snapshot()
	if snapshot == nil {
		return ""
	}
	for _, token := range snapshot.XSTSTokens {
		if token == nil {
			continue
		}
		for _, info := range token.DisplayClaims.UserInfo {
			if info.XUID != "" {
				id := uuid.NewSHA1(serviceDeviceNamespace, []byte(info.XUID))
				return hex.EncodeToString(id[:])
			}
		}
	}
	return ""
}
