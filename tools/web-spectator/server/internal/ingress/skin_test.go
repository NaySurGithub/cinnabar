package ingress

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
	"image"
	"image/png"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestSkinIsBoundToFreshBotAndRevokedWithDuel(t *testing.T) {
	h, store := testHandler(t)
	now := time.Now()
	var pngBytes bytes.Buffer
	if err := png.Encode(&pngBytes, image.NewNRGBA(image.Rect(0, 0, 64, 64))); err != nil {
		t.Fatal(err)
	}
	hash := sha256.Sum256(pngBytes.Bytes())
	id := hex.EncodeToString(hash[:])
	asset := spectator.SkinAsset{Version: 1, ID: "match", PlayerID: "two", SkinID: id, PNG: base64.StdEncoding.EncodeToString(pngBytes.Bytes()), Model: "classic", Width: 64, Height: 64, UpdatedAt: now}
	encoded, _ := json.Marshal(asset)
	if err := store.Accept(spectator.SkinSubject, encoded, now); err != nil {
		t.Fatal(err)
	}
	path := apiPrefix + "/match/skins/" + id
	response := httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
	if response.Code != 404 {
		t.Fatal("unreferenced skin exposed")
	}
	frame, _ := store.Lookup("match").Snapshot(now)
	frame.Players[1].Bot = true
	frame.Players[1].SkinID = id
	// Real native drowning state remains admissible.
	air, maxAir := -19, 300
	frame.Players[1].POV = &spectator.POV{EyeHeight: 1.62, AirTicks: &air, MaxAirTicks: &maxAir}
	encoded, _ = json.Marshal(frame)
	if err := store.Accept(spectator.FrameSubject, encoded, now); err != nil {
		t.Fatal(err)
	}
	response = httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
	if response.Code != 200 || !bytes.Equal(response.Body.Bytes(), pngBytes.Bytes()) || response.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("active bot skin missing or cacheable")
	}
	store.Close("match", now)
	response = httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
	if response.Code != 404 || store.Skin("match", "two", id, now) != nil {
		t.Fatal("closed skin retained")
	}
	asset.ID = "new"
	asset.PNG = base64.StdEncoding.EncodeToString([]byte("invalid PNG"))
	encoded, _ = json.Marshal(asset)
	if store.Accept(spectator.SkinSubject, encoded, now) == nil {
		t.Fatal("invalid PNG/hash accepted")
	}
}
