package showcase

import (
	"errors"
	"math"
)

// Times are seconds on the controller's simulation clock; rates are per second.
const (
	maxStamina        = 100.0
	staminaRegen      = 40.0
	staminaRegenDelay = 0.6
	lightCost         = 12.0
	heavyCost         = 28.0
	dodgeCost         = 22.0
	lightRecovery     = 0.35
	heavyRecovery     = 0.9
	dodgeIFrames      = 0.4
	dodgeCooldown     = 0.5
	doubleTapWindow   = 0.3

	maxEnergy       = 100.0
	chargeRate      = 30.0
	chargeMax       = 6.0
	chargeSlow      = 0.35
	beamMinEnergy   = 10.0
	beamDrain       = 25.0
	beamMax         = 4.0
	flashCost       = 15.0
	flashCooldown   = 2.5
	flashIFrames    = 0.35
	meteorCost      = 30.0
	meteorCooldown  = 8.0
	meteorMinAir    = 0.3
	meteorTimeout   = 3.0
	flightDrain     = 6.0
	startingEnergy  = 50.0
	startingStamina = maxStamina
)

// Ability is one of the player's energy abilities, numbered as the HUD's active field.
type Ability int

const (
	AbilityNone Ability = iota
	AbilityCharge
	AbilityBeam
	AbilityFlash
	AbilityMeteor
	AbilityFlying
)

var (
	errNoStamina  = errors.New("not enough stamina")
	errNoEnergy   = errors.New("not enough energy")
	errCooldown   = errors.New("still on cooldown")
	errBusy       = errors.New("another ability is active")
	errRecovering = errors.New("still recovering from the last attack")
)

// Fighter is one player's combat state: stamina, i-frames, energy and ability timers.
type Fighter struct {
	Stamina, Energy float64

	lastSpend     float64
	recoverUntil  float64
	iframesUntil  float64
	dodgeReadyAt  float64
	lastSneakDown float64

	charging, beaming, leaping bool
	channelStart               float64
	flashReadyAt, meteorReady  float64
	flying                     bool
}

// NewFighter returns a fighter with full stamina and half energy.
func NewFighter() *Fighter {
	return &Fighter{Stamina: startingStamina, Energy: startingEnergy, lastSneakDown: math.Inf(-1)}
}

// Reset restores a fresh fighter, as after respawning at grace.
func (f *Fighter) Reset() { *f = *NewFighter() }

// Invulnerable reports whether a dodge or flash step i-frame window covers now.
func (f *Fighter) Invulnerable(now float64) bool { return now < f.iframesUntil }

func (f *Fighter) spend(cost, now float64) error {
	if f.Stamina <= 0 {
		return errNoStamina
	}
	f.Stamina = max(0, f.Stamina-cost)
	f.lastSpend = now
	return nil
}

// Attack spends stamina for a light or heavy attack; heavy attacks recover longer.
func (f *Fighter) Attack(heavy bool, now float64) error {
	if now < f.recoverUntil {
		return errRecovering
	}
	cost, recovery := lightCost, lightRecovery
	if heavy {
		cost, recovery = heavyCost, heavyRecovery
	}
	if err := f.spend(cost, now); err != nil {
		return err
	}
	f.recoverUntil = now + recovery
	return nil
}

// SneakDown records a sneak press and reports whether it completes a double tap.
func (f *Fighter) SneakDown(now float64) bool {
	double := now-f.lastSneakDown <= doubleTapWindow
	if double {
		f.lastSneakDown = math.Inf(-1)
	} else {
		f.lastSneakDown = now
	}
	return double
}

// Dodge spends stamina and opens the i-frame window.
func (f *Fighter) Dodge(now float64) error {
	if now < f.dodgeReadyAt {
		return errCooldown
	}
	if err := f.spend(dodgeCost, now); err != nil {
		return err
	}
	f.iframesUntil = max(f.iframesUntil, now+dodgeIFrames)
	f.dodgeReadyAt = now + dodgeCooldown
	return nil
}

// Active returns the ability the HUD highlights.
func (f *Fighter) Active() Ability {
	switch {
	case f.charging:
		return AbilityCharge
	case f.beaming:
		return AbilityBeam
	case f.leaping:
		return AbilityMeteor
	case f.flying:
		return AbilityFlying
	}
	return AbilityNone
}

func (f *Fighter) channelling() bool { return f.charging || f.beaming || f.leaping }

// StartCharge begins filling energy.
func (f *Fighter) StartCharge(now float64) error {
	if f.channelling() {
		return errBusy
	}
	f.charging, f.channelStart = true, now
	return nil
}

// StartBeam begins the channelled beam.
func (f *Fighter) StartBeam(now float64) error {
	if f.channelling() {
		return errBusy
	}
	if f.Energy < beamMinEnergy {
		return errNoEnergy
	}
	f.beaming, f.channelStart = true, now
	return nil
}

// StopChannel ends a charge or beam; it reports which one ended.
func (f *Fighter) StopChannel() Ability {
	switch {
	case f.charging:
		f.charging = false
		return AbilityCharge
	case f.beaming:
		f.beaming = false
		return AbilityBeam
	}
	return AbilityNone
}

// Flash spends energy for a flash step and opens its i-frames.
func (f *Fighter) Flash(now float64) error {
	if now < f.flashReadyAt {
		return errCooldown
	}
	if f.charging || f.beaming {
		return errBusy
	}
	if f.Energy < flashCost {
		return errNoEnergy
	}
	f.Energy -= flashCost
	f.flashReadyAt = now + flashCooldown
	f.iframesUntil = max(f.iframesUntil, now+flashIFrames)
	return nil
}

// Meteor spends energy and starts the leap; Land finishes it.
func (f *Fighter) Meteor(now float64) error {
	if now < f.meteorReady {
		return errCooldown
	}
	if f.channelling() {
		return errBusy
	}
	if f.Energy < meteorCost {
		return errNoEnergy
	}
	f.Energy -= meteorCost
	f.meteorReady = now + meteorCooldown
	f.leaping, f.channelStart = true, now
	return nil
}

// Land reports whether touching the ground at now crashes a meteor slam.
func (f *Fighter) Land(onGround bool, now float64) bool {
	if !f.leaping || !onGround || now-f.channelStart < meteorMinAir {
		return false
	}
	f.leaping = false
	return true
}

// Cooldown returns the remaining fraction of an ability's cooldown, 0 when ready.
func (f *Fighter) Cooldown(a Ability, now float64) float64 {
	readyAt, full := 0.0, 1.0
	switch a {
	case AbilityFlash:
		readyAt, full = f.flashReadyAt, flashCooldown
	case AbilityMeteor:
		readyAt, full = f.meteorReady, meteorCooldown
	default:
		return 0
	}
	return clamp01((readyAt - now) / full)
}

// Tick advances regeneration and channels by dt; it returns the channel that ran out, if any.
func (f *Fighter) Tick(now, dt float64, flying bool) Ability {
	f.flying = flying
	if now-f.lastSpend >= staminaRegenDelay {
		f.Stamina = min(maxStamina, f.Stamina+staminaRegen*dt)
	}
	ended := AbilityNone
	switch {
	case f.charging:
		f.Energy = min(maxEnergy, f.Energy+chargeRate*dt)
		if now-f.channelStart >= chargeMax {
			f.charging, ended = false, AbilityCharge
		}
	case f.beaming:
		f.Energy = max(0, f.Energy-beamDrain*dt)
		if f.Energy <= 0 || now-f.channelStart >= beamMax {
			f.beaming, ended = false, AbilityBeam
		}
	case f.leaping:
		if now-f.channelStart >= meteorTimeout {
			f.leaping, ended = false, AbilityMeteor
		}
	}
	if flying {
		f.Energy = max(0, f.Energy-flightDrain*dt)
	}
	return ended
}

// CanFly reports whether the server grants flight.
func (f *Fighter) CanFly() bool { return f.Energy > 0 }

func clamp01(v float64) float64 { return min(1, max(0, v)) }
