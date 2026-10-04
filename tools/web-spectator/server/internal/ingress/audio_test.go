package ingress

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestVerifiedAudioCacheAndDenials(t *testing.T) {
	dir := t.TempDir()
	content := []byte("verified audio bytes")
	hash := sha256.Sum256(content)
	digest := hex.EncodeToString(hash[:])
	name := digest + ".wav"
	if err := os.WriteFile(filepath.Join(dir, name), content, 0600); err != nil {
		t.Fatal(err)
	}
	manifest, err := json.Marshal(struct {
		Version int                    `json:"version"`
		Files   map[string]audioRecord `json:"files"`
	}{Version: 1, Files: map[string]audioRecord{"hit": {SHA256: digest, File: name, Size: int64(len(content))}}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "manifest.json"), manifest, 0600); err != nil {
		t.Fatal(err)
	}
	audio, err := LoadAudio(dir)
	if err != nil {
		t.Fatal(err)
	}
	defer audio.Close()
	h, _ := testHandler(t)
	h.SetAudio(audio)
	manifestResponse := httptest.NewRecorder()
	h.ServeHTTP(manifestResponse, httptest.NewRequest(http.MethodGet, "/api/spectator/audio/manifest", nil))
	if manifestResponse.Code != http.StatusOK || manifestResponse.Header().Get("Cache-Control") != "private, no-store" {
		t.Fatal("audio manifest inherited immutable cache policy")
	}
	route := "/api/spectator/audio/" + digest + "/" + name
	response := httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, route, nil))
	if response.Code != http.StatusOK || response.Header().Get("Cache-Control") != privateImmutableCache || !bytes.Equal(response.Body.Bytes(), content) {
		t.Fatal("verified audio was not privately cacheable")
	}
	conditional := httptest.NewRequest(http.MethodGet, route, nil)
	conditional.Header.Set("If-None-Match", response.Header().Get("ETag"))
	cached := httptest.NewRecorder()
	h.ServeHTTP(cached, conditional)
	if cached.Code != http.StatusNotModified || cached.Header().Get("Cache-Control") != privateImmutableCache || cached.Body.Len() != 0 {
		t.Fatal("audio conditional request lost private immutable policy")
	}
	missing := httptest.NewRecorder()
	h.ServeHTTP(missing, httptest.NewRequest(http.MethodGet, "/api/spectator/audio/missing/file.wav", nil))
	if missing.Code != http.StatusNotFound || missing.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("missing audio was cached")
	}
	h.downloads <- struct{}{}
	h.downloads <- struct{}{}
	busy := httptest.NewRecorder()
	h.ServeHTTP(busy, httptest.NewRequest(http.MethodGet, route, nil))
	if busy.Code != http.StatusTooManyRequests || busy.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("busy audio response was cached")
	}
	<-h.downloads
	<-h.downloads
	h.SetAudio(nil)
	unavailable := httptest.NewRecorder()
	h.ServeHTTP(unavailable, httptest.NewRequest(http.MethodGet, route, nil))
	if unavailable.Code != http.StatusServiceUnavailable || unavailable.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("unavailable audio response was cached")
	}
}
