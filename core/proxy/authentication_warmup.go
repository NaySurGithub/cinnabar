package proxy

import (
	"context"
	"log/slog"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
)

// keepAuthenticationWarm preloads the account-independent token verifier and keeps the account's service
// credential fresh, so a join mints only its key-bound token. Signed out it does nothing; failures are silent.
func keepAuthenticationWarm(ctx context.Context, account *authcache.Account, logger *slog.Logger) (stop func()) {
	if account == nil || account.Closed() {
		return func() {}
	}
	return startAuthenticationKeepWarm(ctx, logger, minecraft.PreloadAuthVerifier, account.KeepFresh)
}

func startAuthenticationKeepWarm(ctx context.Context, logger *slog.Logger, preload func(context.Context) error, keepFresh func(context.Context)) (stop func()) {
	ctx, cancel := context.WithCancel(ctx)
	var workers sync.WaitGroup
	workers.Go(func() {
		started := time.Now()
		err := callSafely("preloading upstream authentication", func() error { return preload(ctx) })
		_ = callSafely("reporting authentication preload", func() error {
			logger.Info("JOIN_AUTH_PRELOAD", "duration_ms", time.Since(started).Seconds()*1000, "success", err == nil)
			return nil
		})
	})
	workers.Go(func() {
		_ = callSafely("refreshing upstream authentication", func() error {
			keepFresh(ctx)
			return nil
		})
	})
	return func() {
		cancel()
		workers.Wait()
	}
}
