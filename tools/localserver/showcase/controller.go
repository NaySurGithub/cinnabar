// Package showcase is the opt-in boss arena of the local server: /showcase souls builds an arena,
// spawns the hollow warden and gives the player stamina combat and energy abilities, with a
// resource pack offered only while a world has the showcase enabled.
package showcase

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/player/bossbar"
	"github.com/df-mc/dragonfly/server/player/scoreboard"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
)

const (
	tickRate = time.Second / 20
	dt       = 0.05
	// MarkerFile in the world directory enables the showcase and records its arena.
	MarkerFile = "showcase.json"
)

// marker is the persisted showcase state of a world.
type marker struct {
	Mode   string `json:"mode"`
	Origin [3]int `json:"origin"`
}

// participant is one player's showcase state.
type participant struct {
	fighter   *Fighter
	hasPack   bool
	hud       string
	bossBar   float64
	moveDir   mgl64.Vec3
	lastPos   mgl64.Vec3
	baseSpeed float64
	baseMode  world.GameMode
	dying     bool
}

// Controller runs the showcase of one world. Every method except Run must be called on that
// world's goroutine, from a transaction, command or player handler.
type Controller struct {
	log      *slog.Logger
	w        *world.World
	dir      string
	packs    *Packs
	transfer string // the address players reconnect to so a newly offered pack loads

	arena *Arena
	now   float64
	// players is written from whichever world a player joins or quits in.
	mu      sync.Mutex
	players map[uuid.UUID]*participant
	flow    Flow
	fight   *bossFight
	seed    uint64
	pending atomic.Bool
}

// NewController returns the showcase of the world in dir, enabled if dir holds a marker from an
// earlier run; transfer is the address players reconnect through to load a newly offered pack.
func NewController(dir, transfer string, log *slog.Logger) (*Controller, error) {
	pack, err := BuildPack()
	if err != nil {
		return nil, fmt.Errorf("build showcase pack: %w", err)
	}
	m, err := readMarker(dir)
	if err != nil {
		return nil, err
	}
	c := &Controller{log: log, dir: dir, packs: NewPacks(pack, m != nil), transfer: transfer, players: map[uuid.UUID]*participant{}}
	if m != nil {
		c.arena = &Arena{Origin: cube.Pos{m.Origin[0], m.Origin[1], m.Origin[2]}}
	}
	return c, nil
}

// Configure registers the boss entity type, offers the pack when enabled and lets the listeners
// offer it later; it must run before conf.New.
func (c *Controller) Configure(conf *server.Config) {
	reg := conf.Entities
	if len(reg.Types()) == 0 {
		reg = entity.DefaultRegistry
	}
	conf.Entities = reg.Config().New(append(slices.Clone(reg.Types()), BossType))
	if c.Enabled() {
		conf.Resources = append(conf.Resources, c.packs.Pack())
	}
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = c.packs.Listener(listen)
	}
}

// Attach binds the showcase to the overworld, before players join or Run starts.
func (c *Controller) Attach(w *world.World) { c.w = w }

// Enabled reports whether the world has the showcase on, so its pack must be offered.
func (c *Controller) Enabled() bool { return c.arena != nil }

// Run ticks the showcase until ctx ends.
func (c *Controller) Run(ctx context.Context) {
	t := time.NewTicker(tickRate)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			// Skip a tick rather than queue them while the world is busy.
			if c.pending.CompareAndSwap(false, true) {
				c.w.Do(func(tx *world.Tx) {
					defer c.pending.Store(false)
					c.tick(tx)
				})
			}
		}
	}
}

// Join attaches the showcase handler to a player who just spawned.
func (c *Controller) Join(p *player.Player) {
	c.mu.Lock()
	c.players[p.UUID()] = &participant{fighter: NewFighter(), hasPack: c.packs.Offered(), lastPos: p.Position()}
	c.mu.Unlock()
	p.Handle(&handler{c: c})
}

// leave undoes the showcase's changes to a player quitting or leaving its world.
func (c *Controller) leave(p *player.Player, quit bool) {
	if part := c.lookup(p.UUID()); part != nil {
		c.restorePlayer(p, part)
	}
	if !quit {
		return
	}
	c.mu.Lock()
	delete(c.players, p.UUID())
	c.mu.Unlock()
}

func (c *Controller) lookup(id uuid.UUID) *participant {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.players[id]
}

// participantOf returns the state of a player in the active arena, or nil.
func (c *Controller) participantOf(p *player.Player) *participant {
	if p.Tx().World() != c.w || c.arena == nil {
		return nil
	}
	return c.lookup(p.UUID())
}

// Enable builds the arena at the player's feet and starts the fight; a player whose session
// predates the pack is sent back through the same address to load it.
func (c *Controller) Enable(tx *world.Tx, p *player.Player) error {
	if err := c.owns(tx); err != nil {
		return err
	}
	arena := Arena{Origin: cube.PosFromVec3(p.Position())}
	if err := writeMarker(c.dir, arena); err != nil {
		return err
	}
	arena.Build(tx)
	c.arena = &arena
	tx.World().SetSpawn(cube.PosFromVec3(arena.PlayerSpawn()))
	c.startFight(tx)
	for e := range tx.Players() {
		q := e.(*player.Player)
		if part := c.lookup(q.UUID()); part != nil {
			c.returnToGrace(q, part)
		}
	}
	giveAbilityItems(p)
	c.packs.Offer()
	if part := c.lookup(p.UUID()); part != nil && !part.hasPack {
		p.Message("§6Loading the showcase pack, reconnecting…")
		addr := c.transfer
		tx.World().DoAfter(time.Second, func(tx *world.Tx) {
			for e := range tx.Players() {
				if q := e.(*player.Player); q.UUID() == p.UUID() {
					if err := q.Transfer(addr); err != nil {
						c.log.Warn("showcase transfer failed", "err", err)
						q.Message("§cRejoin the world to load the showcase pack.")
					}
				}
			}
		})
	}
	return nil
}

// owns reports an error unless tx is the showcase's world.
func (c *Controller) owns(tx *world.Tx) error {
	if tx == nil || tx.World() != c.w {
		return errors.New("the showcase runs in the overworld")
	}
	return nil
}

// Disable stops the showcase; the arena blocks stay, and the pack is no longer offered.
func (c *Controller) Disable(tx *world.Tx) error {
	if err := os.Remove(filepath.Join(c.dir, MarkerFile)); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	c.stopFight(tx)
	for e := range tx.Players() {
		p := e.(*player.Player)
		if part := c.lookup(p.UUID()); part != nil {
			c.restorePlayer(p, part)
			p.RemoveBossBar()
			if part.hud != "" {
				p.RemoveScoreboard()
			}
			part.hud, part.bossBar = "", 0
		}
	}
	c.arena = nil
	c.packs.Withdraw()
	return nil
}

// Reset restarts the fight and returns every player to grace.
func (c *Controller) Reset(tx *world.Tx) {
	if c.arena == nil {
		return
	}
	c.startFight(tx)
	for e := range tx.Players() {
		p := e.(*player.Player)
		if part := c.participantOf(p); part != nil {
			c.returnToGrace(p, part)
		}
	}
}

func (c *Controller) startFight(tx *world.Tx) {
	c.stopFight(tx)
	c.flow.Reset()
	c.seed++
	c.fight = newBossFight(c, tx, NewBoss(c.seed))
}

func (c *Controller) stopFight(tx *world.Tx) {
	if c.fight != nil {
		c.fight.remove(tx)
		c.fight = nil
	}
	// Bosses saved by an earlier run come back without a controller; clear them.
	for e := range tx.Entities() {
		if e.H().Type() == BossType {
			_ = e.Close()
		}
	}
}

func (c *Controller) tick(tx *world.Tx) {
	// A paused local world freezes the fight with it.
	if c.arena == nil || tx.World().Paused() {
		return
	}
	c.now += dt
	present := false
	for e := range tx.Players() {
		p := e.(*player.Player)
		if part := c.participantOf(p); part != nil {
			present = true
			c.tickPlayer(tx, p, part)
		}
	}
	if !present {
		return
	}
	// A restarted server spawns its boss once someone is there to fight it.
	if c.fight == nil {
		c.startFight(tx)
	}
	c.fight.tick(tx)
	if c.flow.Tick(c.now) {
		c.Reset(tx)
	}
}

func (c *Controller) tickPlayer(tx *world.Tx, p *player.Player, part *participant) {
	f := part.fighter
	if d := flat(p.Position().Sub(part.lastPos)); d.Len() > 0.02 {
		part.moveDir = d.Normalize()
	} else {
		part.moveDir = mgl64.Vec3{}
	}
	part.lastPos = p.Position()
	if !part.dying {
		switch f.Tick(c.now, dt, p.Flying()) {
		case AbilityCharge:
			c.endCharge(p, part)
		}
		c.tickAbilities(tx, p, part)
		c.grantFlight(p, part)
	}
	if part.hasPack {
		if s := hudOf(f, p.Health(), p.MaxHealth(), c.now).Encode(); s != part.hud {
			part.hud = s
			p.SendScoreboard(scoreboard.New(s))
		}
	}
	if c.fight != nil {
		if frac := c.fight.boss.HealthFraction(); frac != part.bossBar {
			part.bossBar = frac
			if c.fight.boss.State == BossDead {
				p.RemoveBossBar()
			} else {
				p.SendBossBar(bossbar.New(BossName).WithHealthPercentage(frac).WithColour(bossbar.Red()))
			}
		}
	}
}

// playerDied starts the death screen instead of the vanilla death.
func (c *Controller) playerDied(tx *world.Tx, p *player.Player, part *participant) {
	if !c.flow.PlayerDied(c.now) {
		return
	}
	part.dying = true
	c.endChannels(p, part)
	p.SetImmobile()
	tx.PlaySound(p.Position(), soundNamed("mob.warden.death", 1, 0.6))
	for e := range tx.Players() {
		q := e.(*player.Player)
		if c.participantOf(q) != nil {
			q.SendTitle(deathTitle())
		}
	}
}

// bossFelled shows the victory to everyone in the arena.
func (c *Controller) bossFelled(tx *world.Tx) {
	if !c.flow.BossDied() {
		return
	}
	for e := range tx.Players() {
		p := e.(*player.Player)
		if c.participantOf(p) != nil {
			p.SendTitle(victoryTitle())
			p.PlaySound(soundNamed("random.levelup", 1, 0.8))
			p.Message("§6Rest at the site of grace to face Varr again.")
		}
	}
}

// returnToGrace heals the player and puts them back at the arena entrance.
func (c *Controller) returnToGrace(p *player.Player, part *participant) {
	c.endChannels(p, part)
	part.dying = false
	part.fighter.Reset()
	p.SetMobile()
	p.Heal(p.MaxHealth(), graceHealing{})
	p.Extinguish()
	p.SetVelocity(mgl64.Vec3{})
	p.Teleport(c.arena.PlayerSpawn())
}

// restAtGrace sets the player's spawn to the arena and restarts the fight.
func (c *Controller) restAtGrace(tx *world.Tx, p *player.Player) {
	tx.World().SetSpawn(cube.PosFromVec3(c.arena.PlayerSpawn()))
	tx.PlaySound(c.arena.Grace().Vec3Centre(), soundNamed("beacon.activate", 1, 1.2))
	p.Message("§6Rested at the site of grace.")
	c.Reset(tx)
}

// restorePlayer undoes the showcase's changes to speed and game mode.
func (c *Controller) restorePlayer(p *player.Player, part *participant) {
	c.endChannels(p, part)
	if part.baseMode != nil {
		p.StopFlying()
		p.SetGameMode(part.baseMode)
		part.baseMode = nil
	}
	p.SetMobile()
}

type graceHealing struct{}

func (graceHealing) HealingSource() {}

// bossDamage is the damage source of the boss's attacks.
type bossDamage struct{}

func (bossDamage) ReducedByArmour() bool     { return true }
func (bossDamage) ReducedByResistance() bool { return true }
func (bossDamage) Fire() bool                { return false }
func (bossDamage) IgnoreTotem() bool         { return true }

// isFall reports whether src is fall damage, which the arena waives.
func isFall(src world.DamageSource) bool {
	_, ok := src.(entity.FallDamageSource)
	return ok
}

func readMarker(dir string) (*marker, error) {
	b, err := os.ReadFile(filepath.Join(dir, MarkerFile))
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	var m marker
	if err := json.Unmarshal(b, &m); err != nil {
		return nil, fmt.Errorf("%s: %w", MarkerFile, err)
	}
	return &m, nil
}

func writeMarker(dir string, a Arena) error {
	b, err := json.Marshal(marker{Mode: "souls", Origin: [3]int{a.Origin.X(), a.Origin.Y(), a.Origin.Z()}})
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, MarkerFile), b, 0o644)
}
