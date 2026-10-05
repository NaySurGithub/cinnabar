package showcase

import (
	"image/color"
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/particle"
	"github.com/df-mc/dragonfly/server/world/sound"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	bossWalk       = 0.14 // blocks per tick
	bossWalkPhase2 = 0.2
	bossLunge      = 0.85
	bossStopRange  = 4.2 // keeps the boss beside the player rather than on top of the camera
	knockDecay     = 0.55
	slamRadius     = 5.0
	swipeRadius    = 4.8
	swipeHalfAngle = 70.0
	lungeHitRange  = 2.0
	corpseTime     = 2.0
)

var ember = color.RGBA{R: 255, G: 120, B: 30, A: 255}

// bossFight applies the boss AI to its entity: movement, telegraphs, hits and reactions.
type bossFight struct {
	c        *Controller
	boss     *Boss
	handle   *world.EntityHandle
	mc       entity.MovementComputer
	knock    mgl64.Vec3
	yaw      float64
	lunge    mgl64.Vec3
	lungeOn  map[*world.EntityHandle]bool // players the current lunge already hit
	removeAt float64
}

func newBossFight(c *Controller, tx *world.Tx, b *Boss) *bossFight {
	f := &bossFight{c: c, boss: b, mc: entity.MovementComputer{Gravity: 0.08, Drag: 0.02, DragBeforeGravity: true}}
	f.yaw = 180 // facing the entrance to the south
	f.handle = newBossHandle(c.arena.BossSpawn(), f.yaw)
	tx.AddEntity(f.handle)
	return f
}

func (f *bossFight) entity(tx *world.Tx) (*bossEntity, bool) {
	if f.handle == nil {
		return nil, false
	}
	e, ok := f.handle.Entity(tx)
	if !ok {
		return nil, false
	}
	b, ok := e.(*bossEntity)
	return b, ok
}

func (f *bossFight) alive(*world.Tx) bool { return f.boss.State != BossDead }

func (f *bossFight) remove(tx *world.Tx) {
	if b, ok := f.entity(tx); ok {
		_ = b.Close()
	}
	f.handle = nil
}

// target is the nearest living player in the arena.
func (f *bossFight) target(tx *world.Tx, from mgl64.Vec3) (*player.Player, bool) {
	var best *player.Player
	bestDist := math.Inf(1)
	for e := range tx.Players() {
		p := e.(*player.Player)
		part := f.c.participantOf(p)
		if part == nil || part.dying || !f.c.arena.Contains(p.Position()) {
			continue
		}
		if d := flatDist(p.Position(), from); d < bestDist {
			best, bestDist = p, d
		}
	}
	return best, best != nil
}

func (f *bossFight) tick(tx *world.Tx) {
	b, ok := f.entity(tx)
	if !ok {
		return
	}
	now := f.c.now
	if f.boss.State == BossDead {
		if f.removeAt > 0 && now >= f.removeAt {
			f.remove(tx)
		}
		return
	}
	pos := b.Position()
	target, hasTarget := f.target(tx, pos)
	dist := math.Inf(1)
	if hasTarget {
		dist = flatDist(target.Position(), pos)
		if f.boss.State == BossApproach || f.boss.State == BossWindup {
			f.yaw = yawTowards(pos, target.Position())
		}
	}
	if hasTarget {
		for _, ev := range f.boss.Step(now, dist) {
			f.apply(tx, b, ev, target)
		}
	}
	desired := mgl64.Vec3{}
	switch {
	case f.boss.Lunging():
		desired = f.lunge.Mul(bossLunge)
		f.lungeHits(tx, b)
	case f.boss.Moving() && hasTarget && dist > bossStopRange:
		speed := bossWalk
		if f.boss.Phase == 2 {
			speed = bossWalkPhase2
		}
		desired = yawDir(f.yaw).Mul(speed)
	}
	vel := b.data.Vel
	b.data.Vel = mgl64.Vec3{desired[0] + f.knock[0], vel[1] + f.knock[1], desired[2] + f.knock[2]}
	f.knock = mgl64.Vec3{f.knock[0] * knockDecay, 0, f.knock[2] * knockDecay}
	b.move(tx, &f.mc, f.yaw)
	if f.boss.State == BossStagger && int(now/dt)%4 == 0 {
		tx.AddParticle(b.Position().Add(mgl64.Vec3{0, bossHeight + 0.3, 0}), particle.Note{Instrument: sound.Bass(), Pitch: 2})
	}
}

// apply shows a boss event and resolves its hits.
func (f *bossFight) apply(tx *world.Tx, b *bossEntity, ev BossEvent, target *player.Player) {
	pos := b.Position()
	switch ev {
	case EventWindup:
		f.telegraph(tx, b, target)
	case EventStrike:
		tx.PlayEntityAnimation(b, world.NewEntityAnimation("animation.warden.attack"))
		for _, v := range tx.Viewers(pos) {
			v.ViewEntityAction(b, entity.SwingArmAction{})
		}
		switch f.boss.Attack {
		case AttackSlam:
			spawnMarker(tx, markerSlam, pos)
			tx.PlaySound(pos, soundNamed("mob.warden.attack_impact", 2, 0.6))
			tx.PlaySound(pos, sound.Explosion{})
			ring(tx, pos, slamRadius, 32, particle.BlockBreak{Block: block.StoneBricks{}})
			f.hitPlayers(tx, func(p *player.Player) bool {
				return flatDist(p.Position(), pos) <= slamRadius && p.Position()[1]-pos[1] < 2
			}, pos, 0.7, 0.45)
		case AttackSwipe:
			tx.PlaySound(pos, soundNamed("mob.warden.attack_impact", 1.5, 1.1))
			f.hitPlayers(tx, func(p *player.Player) bool {
				return inArc(pos, f.yaw, p.Position(), swipeRadius, swipeHalfAngle)
			}, pos, 0.6, 0.35)
		case AttackLunge:
			f.lunge = yawDir(f.yaw)
			f.lungeOn = map[*world.EntityHandle]bool{}
			tx.PlaySound(pos, soundNamed("mob.ravager.roar", 1.5, 1.2))
		}
	case EventStagger:
		f.knock = mgl64.Vec3{}
		spawnMarker(tx, markerStagger, pos)
		tx.PlaySound(pos, soundNamed("mob.ravager.stun", 2, 0.8))
	case EventPhase2:
		f.phase2(tx, b)
	case EventDied:
		f.died(tx, b)
	}
}

// telegraph is the wind-up cue: a sound, an animation and a particle outline of the coming hit.
func (f *bossFight) telegraph(tx *world.Tx, b *bossEntity, target *player.Player) {
	pos := b.Position()
	if f.boss.Attack != AttackLunge {
		spawnMarker(tx, markerTelegraph, pos)
	}
	switch f.boss.Attack {
	case AttackSlam:
		tx.PlaySound(pos, soundNamed("mob.warden.sonic_charge", 2, 0.7))
	case AttackSwipe:
		tx.PlaySound(pos, soundNamed("mob.warden.angry", 2, 0.9))
	case AttackLunge:
		tx.PlaySound(pos, soundNamed("mob.warden.roar", 1.5, 1.4))
		to := target.Position()
		dir := flat(to.Sub(pos))
		for d := 1.0; d < dir.Len(); d += 0.8 {
			tx.AddParticle(pos.Add(dir.Normalize().Mul(d)).Add(mgl64.Vec3{0, 0.2, 0}), particle.Dust{Colour: ember})
		}
	}
}

// hitPlayers damages and knocks back every arena player match selects; i-frames dodge it.
func (f *bossFight) hitPlayers(tx *world.Tx, match func(*player.Player) bool, from mgl64.Vec3, force, height float64) {
	for e := range tx.Players() {
		p := e.(*player.Player)
		part := f.c.participantOf(p)
		if part == nil || part.dying || !match(p) {
			continue
		}
		f.hitPlayer(tx, p, part, from, force, height)
	}
}

func (f *bossFight) hitPlayer(tx *world.Tx, p *player.Player, part *participant, from mgl64.Vec3, force, height float64) {
	if part.fighter.Invulnerable(f.c.now) {
		tx.AddParticle(p.Position().Add(mgl64.Vec3{0, 1, 0}), particle.Dust{Colour: color.RGBA{R: 255, G: 255, B: 255, A: 255}})
		p.PlaySound(soundNamed("random.orb", 0.6, 1.8))
		return
	}
	p.Hurt(f.boss.Damage(f.boss.Attack), bossDamage{})
	p.KnockBack(from, force, height)
}

func (f *bossFight) lungeHits(tx *world.Tx, b *bossEntity) {
	pos := b.Position()
	f.hitPlayers(tx, func(p *player.Player) bool {
		if f.lungeOn[p.H()] || flatDist(p.Position(), pos) > lungeHitRange {
			return false
		}
		f.lungeOn[p.H()] = true
		return true
	}, pos, 0.7, 0.4)
}

// hurt damages the boss with a knockback impulse; poise breaks stagger it.
func (f *bossFight) hurt(tx *world.Tx, dmg, poise float64, impulse mgl64.Vec3, heavy bool) {
	b, ok := f.entity(tx)
	if !ok || f.boss.State == BossDead {
		return
	}
	events := f.boss.Hurt(dmg, poise, f.c.now)
	for _, v := range tx.Viewers(b.Position()) {
		v.ViewEntityAction(b, entity.HurtAction{})
	}
	f.knock = f.knock.Add(impulse)
	if heavy {
		tx.PlaySound(b.Position(), soundNamed("mob.warden.hurt", 1.5, 0.8))
	}
	for _, ev := range events {
		f.apply(tx, b, ev, nil)
	}
}

func (f *bossFight) phase2(tx *world.Tx, b *bossEntity) {
	pos := b.Position()
	spawnMarker(tx, markerPhase2, pos)
	tx.PlayEntityAnimation(b, world.NewEntityAnimation("animation.warden.roar"))
	tx.PlaySound(pos, soundNamed("mob.warden.roar", 3, 0.7))
	for e := range tx.Players() {
		p := e.(*player.Player)
		if f.c.participantOf(p) != nil {
			p.SendTitle(phase2Title())
		}
	}
}

func (f *bossFight) died(tx *world.Tx, b *bossEntity) {
	pos := b.Position()
	for _, v := range tx.Viewers(pos) {
		v.ViewEntityAction(b, entity.DeathAction{})
	}
	tx.PlaySound(pos, soundNamed("mob.warden.death", 3, 0.7))
	for r := 1.0; r <= 4; r++ {
		ring(tx, pos.Add(mgl64.Vec3{0, r, 0}), r*0.6, 20, particle.Dust{Colour: gold})
	}
	f.removeAt = f.c.now + corpseTime
	f.c.bossFelled(tx)
}

// ring draws n particles in a horizontal circle.
func ring(tx *world.Tx, centre mgl64.Vec3, radius float64, n int, p world.Particle) {
	for i := range n {
		a := 2 * math.Pi * float64(i) / float64(n)
		tx.AddParticle(centre.Add(mgl64.Vec3{math.Cos(a) * radius, 0.15, math.Sin(a) * radius}), p)
	}
}
