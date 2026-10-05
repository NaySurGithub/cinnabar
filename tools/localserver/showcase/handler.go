package showcase

import (
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/sound"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	lightDamage = 7.0
	heavyDamage = 16.0
	lightPoise  = 8.0
	heavyPoise  = 34.0
	lightKnock  = 0.15
	heavyKnock  = 0.5
	dodgeSpeed  = 1.1
)

// handler translates player events into showcase actions; outside the arena it does nothing.
type handler struct {
	player.NopHandler
	c *Controller
}

func (h *handler) HandleQuit(p *player.Player) { h.c.leave(p) }

func (h *handler) HandleToggleSneak(ctx *player.Context, after bool) {
	p := ctx.Player()
	if part := h.c.participantOf(p); part != nil && after && part.fighter.SneakDown(h.c.now) {
		h.c.dodge(p, part)
	}
}

func (h *handler) HandleJump(p *player.Player) {
	if part := h.c.participantOf(p); part != nil && p.Sneaking() {
		h.c.dodge(p, part)
	}
}

func (h *handler) HandleAttackEntity(ctx *player.Context, e world.Entity, _, _ *float64, _ *bool) {
	p := ctx.Player()
	part := h.c.participantOf(p)
	if part == nil || e.H().Type() != BossType {
		return
	}
	ctx.Cancel()
	if part.dying || h.c.fight == nil || h.c.fight.handle != e.H() {
		return
	}
	heavy := p.Sneaking()
	if part.fighter.Attack(heavy, h.c.now) != nil {
		return
	}
	p.SwingArm()
	held, _ := p.HeldItems()
	dmg, poise, knock := lightDamage, lightPoise, lightKnock
	if heavy {
		dmg, poise, knock = heavyDamage, heavyPoise, heavyKnock
	}
	dmg += held.AttackDamage() - 1
	away := flat(e.Position().Sub(p.Position()))
	if away.Len() > 0 {
		away = away.Normalize()
	}
	tx := p.Tx()
	tx.PlaySound(e.Position(), sound.Attack{Damage: true})
	h.c.fight.hurt(tx, dmg, poise, away.Mul(knock), heavy)
}

func (h *handler) HandlePunchAir(ctx *player.Context) {
	if part := h.c.participantOf(ctx.Player()); part != nil && !part.dying {
		_ = part.fighter.Attack(ctx.Player().Sneaking(), h.c.now)
	}
}

func (h *handler) HandleHurt(ctx *player.Context, dmg *float64, _ bool, _ *time.Duration, src world.DamageSource) {
	p := ctx.Player()
	part := h.c.participantOf(p)
	if part == nil {
		return
	}
	if isFall(src) || part.dying || part.fighter.Invulnerable(h.c.now) {
		ctx.Cancel()
		return
	}
	if *dmg >= p.Health() {
		ctx.Cancel()
		h.c.playerDied(p.Tx(), p, part)
	}
}

func (h *handler) HandleItemUse(ctx *player.Context) {
	p := ctx.Player()
	held, _ := p.HeldItems()
	a, ok := heldAbility(held)
	if !ok || h.c.participantOf(p) == nil {
		return
	}
	ctx.Cancel()
	if err := h.c.Request(p.Tx(), p, a, ""); err != nil {
		p.SendTip("§c" + err.Error())
	}
}

func (h *handler) HandleItemUseOnBlock(ctx *player.Context, pos cube.Pos, _ cube.Face, _ mgl64.Vec3) {
	p := ctx.Player()
	if h.c.participantOf(p) == nil {
		return
	}
	if pos == h.c.arena.Grace() {
		ctx.Cancel()
		h.c.restAtGrace(p.Tx(), p)
		return
	}
	if held, _ := p.HeldItems(); func() bool { _, ok := heldAbility(held); return ok }() {
		ctx.Cancel()
		h.HandleItemUse(ctx)
	}
}

func (h *handler) HandleBlockBreak(ctx *player.Context, pos cube.Pos, _ *[]item.Stack, _ *int) {
	if h.c.participantOf(ctx.Player()) != nil && h.c.arena.Contains(pos.Vec3Centre()) {
		ctx.Cancel()
	}
}

// dodge rolls the player along their movement, or backwards without input, with i-frames.
func (c *Controller) dodge(p *player.Player, part *participant) {
	if part.dying || part.fighter.Dodge(c.now) != nil {
		return
	}
	dir := part.moveDir
	if dir.Len() < 0.5 {
		dir = flat(p.Rotation().Vec3()).Mul(-1)
	}
	if dir.Len() == 0 {
		return
	}
	p.SetVelocity(dir.Normalize().Mul(dodgeSpeed).Add(mgl64.Vec3{0, 0.15, 0}))
	p.Tx().PlaySound(p.Position(), soundNamed("armor.equip_leather", 1, 0.7))
}
