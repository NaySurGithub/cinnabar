package showcase

import (
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// withArena runs f with the showcase enabled around a player standing at 0,10,0.
func withArena(t *testing.T, f func(c *Controller, tx *world.Tx, p *player.Player, part *participant)) {
	t.Helper()
	c, err := NewController(t.TempDir(), "127.0.0.1:19132", slog.New(slog.DiscardHandler))
	if err != nil {
		t.Fatal(err)
	}
	reg := entity.DefaultRegistry.Config().New(append(slices.Clone(entity.DefaultRegistry.Types()), BossType))
	w := world.Config{Synchronous: true, Entities: reg}.New()
	t.Cleanup(func() { _ = w.Close() })
	c.Attach(w)
	task := w.Do(func(tx *world.Tx) {
		handle := world.EntitySpawnOpts{}.New(player.Type, player.Config{Name: "tester", GameMode: world.GameModeSurvival, Position: mgl64.Vec3{0.5, 10, 0.5}})
		p := tx.AddEntity(handle).(*player.Player)
		c.Join(p)
		part := c.lookup(p.UUID())
		part.hasPack = true
		if err := c.Enable(tx, p); err != nil {
			t.Fatal(err)
		}
		f(c, tx, p, part)
	})
	if err := task.Err(); err != nil {
		t.Fatal(err)
	}
}

func ticks(c *Controller, tx *world.Tx, seconds float64) {
	for range int(seconds/dt) + 1 {
		c.tick(tx)
	}
}

func bosses(tx *world.Tx) int {
	n := 0
	for e := range tx.Entities() {
		if e.H().Type() == BossType {
			n++
		}
	}
	return n
}

func TestEnableBuildsTheArenaSpawnsTheBossAndPersists(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, _ *participant) {
		if _, ok := tx.Block(c.arena.Grace()).(block.Campfire); !ok {
			t.Fatalf("grace block is %T", tx.Block(c.arena.Grace()))
		}
		if n := bosses(tx); n != 1 {
			t.Fatalf("%d bosses in the world, want 1", n)
		}
		if p.Position() != c.arena.PlayerSpawn() {
			t.Fatalf("player at %v, want the arena spawn %v", p.Position(), c.arena.PlayerSpawn())
		}
		if held, _ := p.HeldItems(); !func() bool { _, ok := heldAbility(held); return ok }() {
			t.Fatal("no ability item in the first hotbar slot")
		}
		again, err := NewController(c.dir, "", slog.New(slog.DiscardHandler))
		if err != nil || !again.Enabled() || again.arena.Origin != c.arena.Origin {
			t.Fatalf("restart lost the showcase: %v %+v", err, again.arena)
		}
	})
}

func TestLethalHitShowsYouDiedThenRespawnsAtGraceWithTheFightReset(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		c.fight.hurt(tx, 50, 0, mgl64.Vec3{}, false)
		p.Teleport(c.arena.BossSpawn().Add(mgl64.Vec3{0, 0, 6}))
		p.Hurt(1000, bossDamage{})
		if !part.dying || p.Dead() || !p.Immobile() || c.flow.State != FlowDying {
			t.Fatalf("lethal hit: dying=%v dead=%v immobile=%v flow=%v", part.dying, p.Dead(), p.Immobile(), c.flow.State)
		}
		ticks(c, tx, deathScreenTime)
		if part.dying || p.Immobile() || p.Health() != p.MaxHealth() {
			t.Fatalf("after the death screen: dying=%v immobile=%v health=%v", part.dying, p.Immobile(), p.Health())
		}
		if p.Position() != c.arena.PlayerSpawn() {
			t.Fatalf("respawned at %v, want grace %v", p.Position(), c.arena.PlayerSpawn())
		}
		if c.fight.boss.Health != bossMaxHealth || bosses(tx) != 1 {
			t.Fatalf("fight not reset: boss health %v, %d bosses", c.fight.boss.Health, bosses(tx))
		}
	})
}

func TestDodgeIFramesNegateBossHits(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		from := p.Position().Add(mgl64.Vec3{0, 0, -2})
		if err := part.fighter.Dodge(c.now); err != nil {
			t.Fatal(err)
		}
		c.fight.hitPlayer(tx, p, part, from, 1, 0.5)
		if p.Health() != p.MaxHealth() {
			t.Fatalf("hit during i-frames took health to %v", p.Health())
		}
		c.now += dodgeIFrames
		c.fight.hitPlayer(tx, p, part, from, 1, 0.5)
		if p.Health() >= p.MaxHealth() {
			t.Fatal("hit after the i-frames did no damage")
		}
	})
}

func TestFellingTheBossShowsVictoryAndClearsTheCorpse(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		c.fight.hurt(tx, bossMaxHealth*2, 0, mgl64.Vec3{}, true)
		if c.flow.State != FlowWon || c.fight.boss.State != BossDead {
			t.Fatalf("flow %v, boss %v", c.flow.State, c.fight.boss.State)
		}
		if bosses(tx) != 1 {
			t.Fatal("corpse vanished before its death animation")
		}
		ticks(c, tx, corpseTime)
		if bosses(tx) != 0 {
			t.Fatal("corpse left in the arena")
		}
		p.Hurt(1000, bossDamage{})
		if c.flow.State != FlowWon {
			t.Fatal("dying after victory restarted the death flow")
		}
	})
}

func TestRestingAtGraceResetsTheFight(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		c.fight.hurt(tx, 120, 0, mgl64.Vec3{}, false)
		part.fighter.Stamina = 0
		c.restAtGrace(tx, p)
		if c.fight.boss.Health != bossMaxHealth || part.fighter.Stamina != maxStamina || bosses(tx) != 1 {
			t.Fatalf("rest: boss %v stamina %v bosses %d", c.fight.boss.Health, part.fighter.Stamina, bosses(tx))
		}
	})
}

func TestAbilityRequestsAreValidated(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		if err := c.Request(tx, p, AbilityFlash, "sideways"); !errors.Is(err, errPhase) {
			t.Fatalf("bad phase = %v", err)
		}
		if err := c.Request(tx, p, AbilityMeteor, ""); err != nil {
			t.Fatal(err)
		}
		if p.Velocity()[1] <= 0 {
			t.Fatal("meteor slam did not leap")
		}
		if err := c.Request(tx, p, AbilityMeteor, "start"); !errors.Is(err, errCooldown) && !errors.Is(err, errBusy) {
			t.Fatalf("second meteor = %v, want cooldown or busy", err)
		}
		part.fighter.Energy = 0
		if err := c.Request(tx, p, AbilityFlash, ""); !errors.Is(err, errNoEnergy) {
			t.Fatalf("flash without energy = %v", err)
		}
		part.fighter.leaping = false
		part.fighter.Energy = 50
		speed := p.Speed()
		if err := c.Request(tx, p, AbilityCharge, "start"); err != nil || p.Speed() >= speed {
			t.Fatalf("charge: %v, speed %v -> %v", err, speed, p.Speed())
		}
		if err := c.Request(tx, p, AbilityCharge, "stop"); err != nil || p.Speed() != speed || part.fighter.Active() != AbilityNone {
			t.Fatalf("charge stop: %v, speed %v, active %v", err, p.Speed(), part.fighter.Active())
		}
		part.dying = true
		if err := c.Request(tx, p, AbilityBeam, "start"); !errors.Is(err, errDown) {
			t.Fatalf("beam while dying = %v", err)
		}
	})
}

func TestMeteorCrashHurtsAndThrowsANearbyBoss(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		p.Teleport(c.arena.BossSpawn().Add(mgl64.Vec3{0, 0, 3}))
		c.meteorCrash(tx, p)
		if c.fight.boss.Health != bossMaxHealth-meteorDamage {
			t.Fatalf("boss health %v after a meteor crash", c.fight.boss.Health)
		}
		if c.fight.knock.Len() == 0 {
			t.Fatal("meteor crash did not knock the boss back")
		}
	})
}

func TestArenaWaivesFallDamage(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		p.Hurt(6, entity.FallDamageSource{})
		if p.Health() != p.MaxHealth() {
			t.Fatalf("fall damage applied: %v", p.Health())
		}
	})
}

func TestShowcaseOffWithdrawsThePackAndTheBoss(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		c.packs.Offer()
		part.hud = "" // a test player's Nop session cannot remove a scoreboard
		if err := c.Disable(tx); err != nil {
			t.Fatal(err)
		}
		if c.Enabled() || c.packs.Offered() || bosses(tx) != 0 {
			t.Fatalf("enabled=%v offered=%v bosses=%d", c.Enabled(), c.packs.Offered(), bosses(tx))
		}
		if _, err := os.Stat(filepath.Join(c.dir, MarkerFile)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("marker still present: %v", err)
		}
		if err := c.Request(tx, p, AbilityFlash, ""); !errors.Is(err, errNoShow) {
			t.Fatalf("ability after showcase off = %v", err)
		}
	})
}
