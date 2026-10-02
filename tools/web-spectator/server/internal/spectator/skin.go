package spectator

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"image/png"
	"time"
)

const maxSkinBytes = 8 << 20
const skinLifetime = 35 * time.Second

type SkinAsset struct {
	Version   int       `json:"version"`
	ID        string    `json:"id"`
	PlayerID  string    `json:"playerId"`
	SkinID    string    `json:"skinId"`
	PNG       string    `json:"png"`
	Model     string    `json:"model"`
	Width     int       `json:"width"`
	Height    int       `json:"height"`
	UpdatedAt time.Time `json:"updatedAt"`
}
type skinKey struct{ match, player, hash string }
type skinEntry struct {
	png     []byte
	updated time.Time
}

func validSkinID(id string) bool {
	if len(id) != 64 {
		return false
	}
	for _, r := range id {
		if !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f') {
			return false
		}
	}
	return true
}
func validSkinModel(model string) bool {
	return model == "classic" || model == "slim" || model == "unknown"
}
func (s *Store) acceptSkin(asset SkinAsset, now time.Time) error {
	if asset.Version != Version || !ValidID(asset.ID) || !cleanLabel(asset.PlayerID, 128) || !validSkinID(asset.SkinID) || !validSkinModel(asset.Model) || !fresh(asset.UpdatedAt, now) || !(asset.Width == 64 || asset.Width == 128 || asset.Width == 256) || !(asset.Height == asset.Width || asset.Height == asset.Width/2) {
		return errors.New("invalid spectator skin")
	}
	data, err := base64.StdEncoding.DecodeString(asset.PNG)
	if err != nil || len(data) == 0 || len(data) > 512<<10 {
		return errors.New("invalid spectator PNG")
	}
	digest := sha256.Sum256(data)
	if hex.EncodeToString(digest[:]) != asset.SkinID {
		return errors.New("spectator skin hash mismatch")
	}
	config, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil || config.Width != asset.Width || config.Height != asset.Height {
		return errors.New("spectator skin dimensions mismatch")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, closed := s.closed[asset.ID]; closed || now.Before(s.blockedUntil) {
		return errors.New("spectator skin match closed")
	}
	key := skinKey{asset.ID, asset.PlayerID, asset.SkinID}
	if entry, found := s.skins[key]; found {
		entry.updated = now
		s.skins[key] = entry
		return nil
	}
	if len(s.skins) >= 256 || s.skinBytes+len(data) > maxSkinBytes {
		return errors.New("spectator skin cache full")
	}
	// Publications may precede their first frame after reconnect. Bound these
	// pending entries by the same lifetime and byte budget as active skins.
	s.skins[key] = skinEntry{png: data, updated: now}
	s.skinBytes += len(data)
	return nil
}
func (s *Store) Skin(match, player, hash string, now time.Time) []byte {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry := s.skins[skinKey{match, player, hash}]
	if now.Sub(entry.updated) > skinLifetime {
		return nil
	}
	return entry.png // immutable after insertion
}
func (s *Store) discardSkins(match string) {
	for key, entry := range s.skins {
		if key.match == match {
			delete(s.skins, key)
			s.skinBytes -= len(entry.png)
		}
	}
}
