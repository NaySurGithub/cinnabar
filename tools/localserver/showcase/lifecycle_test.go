package showcase

import (
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

func TestPausedWorldFreezesTheShowcase(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		tx.World().SetPaused(true)
		before := c.now
		b, _ := c.fight.entity(tx)
		pos := b.Position()
		part.fighter.Stamina = 0
		ticks(c, tx, 3)
		if c.now != before || part.fighter.Stamina != 0 || b.Position() != pos {
			t.Fatalf("paused world ticked: now %v->%v stamina %v boss %v->%v", before, c.now, part.fighter.Stamina, pos, b.Position())
		}
	})
}

// inOtherWorld runs f with a player standing in a second world the controller does not own.
func inOtherWorld(t *testing.T, f func(tx *world.Tx, p *player.Player)) {
	t.Helper()
	reg := entity.DefaultRegistry.Config().New(append(slices.Clone(entity.DefaultRegistry.Types()), BossType))
	w := world.Config{Synchronous: true, Entities: reg, Dim: world.Nether}.New()
	t.Cleanup(func() { _ = w.Close() })
	if err := w.Do(func(tx *world.Tx) {
		handle := world.EntitySpawnOpts{}.New(player.Type, player.Config{Name: "elsewhere", Position: mgl64.Vec3{0, 70, 0}})
		f(tx, tx.AddEntity(handle).(*player.Player))
	}).Err(); err != nil {
		t.Fatal(err)
	}
}

func TestResetAndOffRejectOtherWorlds(t *testing.T) {
	withArena(t, func(c *Controller, _ *world.Tx, _ *player.Player, _ *participant) {
		c.fight.boss.Health = 10
		inOtherWorld(t, func(tx *world.Tx, q *player.Player) {
			for _, r := range []cmd.Runnable{showcaseReset{c: c}, showcaseOff{c: c}} {
				o := &cmd.Output{}
				r.Run(q, o, tx)
				if o.ErrorCount() == 0 {
					t.Errorf("%T ran from another world", r)
				}
			}
		})
		if !c.Enabled() || c.fight.boss.Health != 10 {
			t.Fatalf("other-world command changed the fight: enabled=%v boss=%v", c.Enabled(), c.fight.boss.Health)
		}
	})
}

func TestEveryEnableOffersThePack(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, _ *participant) {
		if !c.packs.Offered() {
			t.Fatal("pack not offered after /showcase souls")
		}
		if err := c.Disable(tx); err != nil {
			t.Fatal(err)
		}
		if err := c.Enable(tx, p); err != nil {
			t.Fatal(err)
		}
		if !c.packs.Offered() {
			t.Fatal("pack not offered after /showcase off then souls")
		}
	})
}

func TestQuitAndWorldChangeRestoreAbilityOverrides(t *testing.T) {
	for _, leave := range []struct {
		name string
		do   func(h player.Handler, c *Controller, p *player.Player)
	}{
		{"quit", func(h player.Handler, _ *Controller, p *player.Player) { h.HandleQuit(p) }},
		{"change world", func(h player.Handler, c *Controller, p *player.Player) { h.HandleChangeWorld(p, c.w, nil) }},
	} {
		t.Run(leave.name, func(t *testing.T) {
			withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
				speed := p.Speed()
				c.tick(tx)
				if _, ok := p.GameMode().(energyFlight); !ok {
					t.Fatalf("energy flight not granted: %T", p.GameMode())
				}
				if err := c.Request(tx, p, AbilityCharge, "start"); err != nil {
					t.Fatal(err)
				}
				leave.do(p.Handler(), c, p)
				if p.GameMode() != world.GameModeSurvival || p.Speed() != speed {
					t.Fatalf("after %s: mode %T speed %v, want survival and %v", leave.name, p.GameMode(), p.Speed(), speed)
				}
			})
		})
	}
}

func TestEnableDuringTheDeathScreenClearsIt(t *testing.T) {
	withArena(t, func(c *Controller, tx *world.Tx, p *player.Player, part *participant) {
		p.Hurt(1000, bossDamage{})
		if !part.dying {
			t.Fatal("setup: player not dying")
		}
		if err := c.Enable(tx, p); err != nil {
			t.Fatal(err)
		}
		if part.dying || p.Immobile() || c.flow.State != FlowFighting {
			t.Fatalf("after enable: dying=%v immobile=%v flow=%v", part.dying, p.Immobile(), c.flow.State)
		}
		if err := c.Request(tx, p, AbilityFlash, ""); err != nil {
			t.Fatalf("player still locked out: %v", err)
		}
	})
}
