package main

import (
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/cinema"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
)

// startClientParts sets up the server half of client parts for the -extension flags: it reads
// the server key and the bundles, takes the next revision, signs the offer and writes it as an
// optional resource pack, so it must run before server.UserConfig.Config loads the packs.
func startClientParts(cfg settings, log *slog.Logger) (*extension.Server, error) {
	seed, err := os.ReadFile(cfg.extensionKey)
	if err != nil {
		return nil, fmt.Errorf("read -extension-key: %w", err)
	}
	key, err := extension.ParseSeed(string(seed))
	if err != nil {
		return nil, fmt.Errorf("-extension-key %s: %w", cfg.extensionKey, err)
	}
	bundles, err := extension.ReadBundles(cfg.extensionCXB)
	if err != nil {
		return nil, fmt.Errorf("-extension-cxb: %w", err)
	}
	revision, err := extension.NextRevision(filepath.Join(cfg.dir, extension.RevisionFile))
	if err != nil {
		return nil, err
	}
	var mediaOrigins []string
	if cfg.extensionMedia != "" {
		origin, err := cinema.Origin(cfg.extensionMediaAddr)
		if err != nil {
			return nil, err
		}
		mediaOrigins = []string{origin}
	}
	ext, err := extension.NewServer(extension.Config{
		Key:          key,
		Audience:     cfg.extensionAudience,
		Revision:     revision,
		Bundles:      bundles,
		MediaOrigins: mediaOrigins,
		Log:          log,
	})
	if err != nil {
		return nil, fmt.Errorf("-extension-audience or -extension-cxb: %w", err)
	}
	if err := ext.WriteMarkerPack(cfg.resourcesDir()); err != nil {
		return nil, fmt.Errorf("write the client part offer: %w", err)
	}
	offer := ext.Offer()
	ids := make([]string, len(offer.Packages))
	for i, p := range offer.Packages {
		ids[i] = p.ID
	}
	log.Info("client parts offered", "audience", offer.Audience, "revision", offer.Revision,
		"expires", time.Unix(int64(offer.ExpiresUnix), 0).UTC(), "packages", ids)
	return ext, nil
}

// startCinema serves -extension-media on loopback HTTPS, writing its CA into the world directory,
// and installs the intro Cinema timed by the showcase bundle's descriptor.
func startCinema(cfg settings, ext *extension.Server, log *slog.Logger) (*cinema.Cinema, *cinema.MediaServer, error) {
	duration, err := cinema.Duration(cfg.extensionCXB)
	if err != nil {
		return nil, nil, fmt.Errorf("-extension-media: %w", err)
	}
	caPath := filepath.Join(cfg.dir, cinema.CAFile)
	media, err := cinema.ServeMedia(cfg.extensionMedia, cfg.extensionMediaAddr, caPath, log)
	if err != nil {
		return nil, nil, err
	}
	c := cinema.New(ext, duration)
	cinema.SetDefault(c)
	log.Info("client part media served", "addr", cfg.extensionMediaAddr, "ca", caPath, "intro", duration)
	return c, media, nil
}

// deliverClientMessages passes each client part message to the Experience whose id is the bundle
// id, as that player's callback, except the intro screen's events, which go to cin; a player who
// has left, or a server without that Experience, drops it.
func deliverClientMessages(ext *extension.Server, players func(uuid.UUID) (*world.EntityHandle, bool), host *experience.Host, cin *cinema.Cinema, log *slog.Logger) {
	ext.OnClientMessage(func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) {
		if exp == cinema.BundleID && cin != nil && cin.Receive(player, channel, schema, payload) {
			return
		}
		handle, ok := players(player)
		if !ok || host == nil || !host.DeliverClientMessage(handle, exp, channel, schema, payload) {
			log.Debug("client part message dropped", "experience", exp, "channel", channel, "schema", schema)
		}
	})
}
