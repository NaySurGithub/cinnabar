package showcase

import (
	"archive/zip"
	"bytes"
	"embed"
	"encoding/json"
	"hash/crc32"
	"io/fs"
	"maps"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/df-mc/dragonfly/server"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

const (
	// PackUUID is the showcase pack's header UUID, recorded in the shared showcase spec.
	PackUUID   = "0163ebd6-b795-4d03-b532-47b0a33d673b"
	moduleUUID = "f07fd988-f1dd-4463-8b7b-878f6c241711"
	packName   = "Cinnabar Showcase"
)

//go:embed all:pack
var packFiles embed.FS

// BuildPack assembles the pack: the embedded definitions, the generated textures and a manifest
// whose patch version follows the content, so a changed pack is never served from a stale cache.
func BuildPack() (*resource.Pack, error) {
	files := map[string][]byte{}
	err := fs.WalkDir(packFiles, "pack", func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		b, err := packFiles.ReadFile(path)
		files[path[len("pack/"):]] = b
		return err
	})
	if err != nil {
		return nil, err
	}
	body, glow, err := bossTextures()
	if err != nil {
		return nil, err
	}
	files["textures/entity/hollow_warden.png"] = body
	files["textures/entity/hollow_warden_glow.png"] = glow
	if files["textures/ui/cinnabar_solid.png"], err = solidTexture(); err != nil {
		return nil, err
	}

	sum := crc32.NewIEEE()
	for _, name := range slices.Sorted(maps.Keys(files)) {
		sum.Write([]byte(name))
		sum.Write(files[name])
	}
	version := [3]int{1, 0, int(sum.Sum32() % 1_000_000)}
	manifest, err := json.MarshalIndent(map[string]any{
		"format_version": 2,
		"header": map[string]any{
			"name": packName, "description": "Boss showcase art and HUD for the local server",
			"uuid": PackUUID, "version": version, "min_engine_version": [3]int{1, 21, 0},
		},
		"modules": []map[string]any{{"type": "resources", "uuid": moduleUUID, "version": version}},
	}, "", "  ")
	if err != nil {
		return nil, err
	}
	files["manifest.json"] = manifest

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for _, name := range slices.Sorted(maps.Keys(files)) {
		w, err := zw.Create(name)
		if err != nil {
			return nil, err
		}
		if _, err := w.Write(files[name]); err != nil {
			return nil, err
		}
	}
	if err := zw.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(buf.Bytes())
}

// packHolder is the part of a Dragonfly listener that changes the offered packs.
type packHolder interface {
	AddResourcePack(*resource.Pack)
	RemoveResourcePack(uuid string)
}

// Packs offers the showcase pack on the server's listeners while a world has the showcase enabled.
type Packs struct {
	pack    *resource.Pack
	offered atomic.Bool

	mu        sync.Mutex
	listeners []packHolder
}

// NewPacks returns the offer state; offered is true when the pack is already in the server config.
func NewPacks(pack *resource.Pack, offered bool) *Packs {
	p := &Packs{pack: pack}
	p.offered.Store(offered)
	return p
}

// Pack is the showcase pack.
func (p *Packs) Pack() *resource.Pack { return p.pack }

// Listener wraps a listener constructor so the pack can be offered on it later.
func (p *Packs) Listener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		l, err := inner(conf)
		if h, ok := l.(packHolder); ok && err == nil {
			p.mu.Lock()
			p.listeners = append(p.listeners, h)
			p.mu.Unlock()
		}
		return l, err
	}
}

// Offered reports whether sessions starting now receive the pack.
func (p *Packs) Offered() bool { return p.offered.Load() }

// Offer adds the pack for new sessions.
func (p *Packs) Offer() {
	if p.offered.Swap(true) {
		return
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	for _, l := range p.listeners {
		l.AddResourcePack(p.pack)
	}
}

// Withdraw stops offering the pack to new sessions.
func (p *Packs) Withdraw() {
	if !p.offered.Swap(false) {
		return
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	for _, l := range p.listeners {
		l.RemoveResourcePack(PackUUID)
	}
}
