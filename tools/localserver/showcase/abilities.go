package showcase

import (
	"errors"
	"fmt"
	"image/color"
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/particle"
	"github.com/df-mc/dragonfly/server/world/sound"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	beamRange      = 24.0
	beamDamage     = 1.2 // per tick
	beamPoise      = 1.5
	beamKnock      = 0.05
	flashDistance  = 8.0
	meteorRadius   = 6.0
	meteorDamage   = 30.0
	meteorPoise    = 60.0
	meteorKnock    = 1.2
	meteorLeapUp   = 1.25
	meteorLeapFwd  = 1.1
	abilityItemKey = "showcase_ability"
)

var (
	gold      = color.RGBA{R: 255, G: 196, B: 64, A: 255}
	beamBlue  = color.RGBA{R: 140, G: 220, B: 255, A: 255}
	errPhase  = errors.New("unknown phase: use start or stop")
	errNoShow = errors.New("the showcase is not running here")
	errDown   = errors.New("you cannot act right now")
)

// abilityNames maps the /ability verbs to abilities.
var abilityNames = map[string]Ability{"charge": AbilityCharge, "beam": AbilityBeam, "flash": AbilityFlash, "meteor": AbilityMeteor}

// Request runs one validated ability request: phase is "start", "stop" or empty (a tap, or a toggle for held abilities).
func (c *Controller) Request(tx *world.Tx, p *player.Player, a Ability, phase string) error {
	part := c.participantOf(p)
	if part == nil {
		return errNoShow
	}
	if part.dying || p.Immobile() {
		return errDown
	}
	f := part.fighter
	switch phase {
	case "", "start", "stop":
	default:
		return errPhase
	}
	held := a == AbilityCharge || a == AbilityBeam
	if held && (phase == "stop" || (phase == "" && f.Active() == a)) {
		if f.Active() == a {
			c.endChannels(p, part)
		}
		return nil
	}
	if phase == "stop" {
		return nil
	}
	switch a {
	case AbilityCharge:
		if err := f.StartCharge(c.now); err != nil {
			return err
		}
		part.baseSpeed = p.Speed()
		p.SetSpeed(part.baseSpeed * chargeSlow)
		tx.PlaySound(p.Position(), soundNamed("beacon.power", 1, 1.4))
	case AbilityBeam:
		if err := f.StartBeam(c.now); err != nil {
			return err
		}
		tx.PlaySound(p.Position(), soundNamed("mob.warden.sonic_charge", 1, 1.6))
	case AbilityFlash:
		if err := f.Flash(c.now); err != nil {
			return err
		}
		c.flashStep(tx, p, part)
	case AbilityMeteor:
		if err := f.Meteor(c.now); err != nil {
			return err
		}
		look := flat(p.Rotation().Vec3())
		if look.Len() > 0 {
			look = look.Normalize()
		}
		p.SetVelocity(look.Mul(meteorLeapFwd).Add(mgl64.Vec3{0, meteorLeapUp, 0}))
		tx.PlaySound(p.Position(), sound.FireCharge{})
	default:
		return fmt.Errorf("unknown ability %d", a)
	}
	return nil
}

// tickAbilities runs the channelled effects and lands meteor slams.
func (c *Controller) tickAbilities(tx *world.Tx, p *player.Player, part *participant) {
	f := part.fighter
	pos := p.Position()
	tick := int(math.Round(c.now / dt))
	switch f.Active() {
	// Ability visuals belong to the client effects mod; the server keeps only sounds and debris.
	case AbilityCharge:
		if tick%20 == 0 {
			tx.PlaySound(pos, soundNamed("beacon.ambient", 1, 1.6))
		}
	case AbilityBeam:
		c.beamTick(tx, p, tick)
	case AbilityMeteor:
		if f.Land(p.OnGround(), c.now) {
			c.meteorCrash(tx, p)
		}
	}
}

// endCharge restores the speed charging slowed.
func (c *Controller) endCharge(p *player.Player, part *participant) {
	if part.baseSpeed > 0 {
		p.SetSpeed(part.baseSpeed)
		part.baseSpeed = 0
	}
}

// endChannels stops a charge or beam in progress.
func (c *Controller) endChannels(p *player.Player, part *participant) {
	if part.fighter.StopChannel() == AbilityCharge || part.baseSpeed > 0 {
		c.endCharge(p, part)
	}
}

// grantFlight allows flight while the player has energy, double-tap jump toggling it as usual.
func (c *Controller) grantFlight(p *player.Player, part *participant) {
	can := part.fighter.CanFly()
	switch {
	case can && part.baseMode == nil && !p.GameMode().AllowsFlying():
		part.baseMode = p.GameMode()
		p.SetGameMode(energyFlight{part.baseMode})
	case !can && part.baseMode != nil:
		p.StopFlying()
		p.SetGameMode(part.baseMode)
		part.baseMode = nil
	}
}

// energyFlight lets a game mode without flight fly while the player has energy.
type energyFlight struct{ world.GameMode }

func (energyFlight) AllowsFlying() bool { return true }

// beamTick damages the boss along the player's look ray and draws the beam.
func (c *Controller) beamTick(tx *world.Tx, p *player.Player, tick int) {
	eye := p.Position().Add(mgl64.Vec3{0, p.EyeHeight(), 0})
	dir := p.Rotation().Vec3().Normalize()
	length := blockDistance(tx, eye, dir, beamRange)
	if c.fight != nil && c.fight.alive(tx) {
		if b, ok := c.fight.entity(tx); ok {
			if t, hit := rayBox(eye, dir, b.bbox(), length); hit {
				length = t
				c.fight.hurt(tx, beamDamage, beamPoise, dir.Mul(beamKnock), false)
			}
		}
	}
	if tick%10 == 0 {
		tx.PlaySound(eye, soundNamed("beacon.ambient", 1, 2))
	}
}

// flashStep teleports up to flashDistance along the movement direction, stopping before walls.
func (c *Controller) flashStep(tx *world.Tx, p *player.Player, part *participant) {
	dir := part.moveDir
	if dir.Len() < 0.5 {
		dir = flat(p.Rotation().Vec3())
	}
	if dir.Len() == 0 {
		return
	}
	dir = dir.Normalize()
	start := p.Position()
	dest := start
	for d := 0.5; d <= flashDistance; d += 0.5 {
		next := start.Add(dir.Mul(d))
		if !passable(tx, next) {
			break
		}
		dest = next
	}
	p.Teleport(dest)
	tx.PlaySound(dest, sound.Teleport{})
}

// meteorCrash is the meteor slam's landing: an AoE that hurts and throws the boss.
func (c *Controller) meteorCrash(tx *world.Tx, p *player.Player) {
	pos := p.Position()
	tx.PlaySound(pos, sound.Explosion{})
	floor := tx.Block(cube.PosFromVec3(pos).Side(cube.FaceDown))
	for r := 1.5; r <= meteorRadius; r += 1.5 {
		for i := range 16 {
			a := float64(i) * math.Pi / 8
			at := pos.Add(mgl64.Vec3{math.Cos(a) * r, 0.1, math.Sin(a) * r})
			tx.AddParticle(at, particle.BlockBreak{Block: floor})
		}
	}
	if c.fight == nil {
		return
	}
	if b, ok := c.fight.entity(tx); ok && c.fight.alive(tx) && flatDist(b.Position(), pos) <= meteorRadius+bossHalfWidth {
		away := flat(b.Position().Sub(pos))
		if away.Len() > 0 {
			away = away.Normalize()
		}
		c.fight.hurt(tx, meteorDamage, meteorPoise, away.Mul(meteorKnock).Add(mgl64.Vec3{0, 0.4, 0}), true)
	}
}

// blockDistance is how far a ray travels before entering a non-air block, up to maxDist.
func blockDistance(tx *world.Tx, origin, dir mgl64.Vec3, maxDist float64) float64 {
	for d := 0.25; d < maxDist; d += 0.25 {
		if _, air := tx.Block(cube.PosFromVec3(origin.Add(dir.Mul(d)))).(block.Air); !air {
			return d
		}
	}
	return maxDist
}

// passable reports whether a player could stand with their feet at pos.
func passable(tx *world.Tx, pos mgl64.Vec3) bool {
	feet := cube.PosFromVec3(pos)
	for _, at := range []cube.Pos{feet, feet.Side(cube.FaceUp)} {
		if _, air := tx.Block(at).(block.Air); !air {
			return false
		}
	}
	return true
}

// abilityItems are the hotbar fallbacks for players without the client mod, in slot order.
var abilityItems = []struct {
	name string
	it   world.Item
	verb string
}{
	{"§6Charge §7(use: toggle)", item.BlazePowder{}, "charge"},
	{"§bEnergy Beam §7(use: toggle)", item.AmethystShard{}, "beam"},
	{"§fFlash Step", item.Feather{}, "flash"},
	{"§cMeteor Slam", item.FireCharge{}, "meteor"},
}

func giveAbilityItems(p *player.Player) {
	for slot, a := range abilityItems {
		_ = p.Inventory().SetItem(slot, item.NewStack(a.it, 1).WithCustomName(a.name).WithValue(abilityItemKey, a.verb))
	}
}

// heldAbility returns the ability of an ability item.
func heldAbility(s item.Stack) (Ability, bool) {
	v, ok := s.Value(abilityItemKey)
	if !ok {
		return 0, false
	}
	verb, _ := v.(string)
	a, ok := abilityNames[verb]
	return a, ok
}
