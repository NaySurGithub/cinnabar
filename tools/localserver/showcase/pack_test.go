package showcase

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"io"
	"regexp"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func packEntries(t *testing.T, p *resource.Pack) map[string][]byte {
	t.Helper()
	data := make([]byte, p.Len())
	if _, err := p.ReadAt(data, 0); err != nil && err != io.EOF {
		t.Fatal(err)
	}
	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		t.Fatal(err)
	}
	files := map[string][]byte{}
	for _, f := range zr.File {
		r, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		files[f.Name], _ = io.ReadAll(r)
		_ = r.Close()
	}
	return files
}

var lineComment = regexp.MustCompile(`(?m)^\s*//.*$`)

func TestPackCarriesTheBossAndHUDWithAStableIdentity(t *testing.T) {
	p, err := BuildPack()
	if err != nil {
		t.Fatal(err)
	}
	if p.UUID().String() != PackUUID || !p.HasTextures() {
		t.Fatalf("pack %v textures=%v", p.UUID(), p.HasTextures())
	}
	files := packEntries(t, p)
	for _, name := range []string{
		"entity/hollow_warden.entity.json", "render_controllers/hollow_warden.render_controllers.json",
		"ui/hud_screen.json", "textures/entity/hollow_warden.png", "textures/entity/hollow_warden_glow.png",
		"textures/ui/cinnabar_solid.png",
	} {
		if len(files[name]) == 0 {
			t.Errorf("pack lacks %s", name)
		}
	}
	for name, b := range files {
		if bytes.HasSuffix([]byte(name), []byte(".json")) && !json.Valid(lineComment.ReplaceAll(b, nil)) {
			t.Errorf("%s is not valid JSON", name)
		}
	}
	var entity struct {
		Def struct {
			Description struct{ Identifier string } `json:"description"`
		} `json:"minecraft:client_entity"`
	}
	if err := json.Unmarshal(files["entity/hollow_warden.entity.json"], &entity); err != nil || entity.Def.Description.Identifier != BossIdentifier {
		t.Fatalf("client entity identifier %q (%v), want %q", entity.Def.Description.Identifier, err, BossIdentifier)
	}
	again, err := BuildPack()
	if err != nil || again.Version() != p.Version() {
		t.Fatalf("rebuilt pack version %v, want the same %v", again.Version(), p.Version())
	}
}
