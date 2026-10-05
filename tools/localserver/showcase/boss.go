package showcase

import "math/rand/v2"

const (
	bossMaxHealth   = 400.0
	bossMaxPoise    = 100.0
	staggerTime     = 1.2
	roarTime        = 2.0
	phase2Speed     = 0.7 // multiplies wind-up and recovery times
	phase2Damage    = 1.25
	meleeRange      = 4.5
	lungeRange      = 13.0
	phase2Threshold = 0.5
)

// BossState is the boss AI's current state.
type BossState int

const (
	BossApproach BossState = iota
	BossWindup
	BossStrike
	BossRecover
	BossStagger
	BossRoar
	BossDead
)

// Attack is one of the boss's telegraphed attacks.
type Attack int

const (
	AttackSlam Attack = iota
	AttackSwipe
	AttackLunge
)

// attackTiming is an attack's wind-up, active and recovery durations at phase 1.
type attackTiming struct{ windup, active, recover float64 }

var timings = map[Attack]attackTiming{
	AttackSlam:  {windup: 1.1, active: 0.15, recover: 1.0},
	AttackSwipe: {windup: 0.7, active: 0.15, recover: 0.7},
	AttackLunge: {windup: 0.8, active: 0.45, recover: 0.9},
}

// attackDamage is the phase 1 damage each attack deals to a player it hits.
var attackDamage = map[Attack]float64{AttackSlam: 9, AttackSwipe: 6, AttackLunge: 7}

// BossEvent is something the controller must show or apply.
type BossEvent int

const (
	EventWindup BossEvent = iota // telegraph Boss.Attack
	EventStrike                  // apply Boss.Attack's hit
	EventStagger
	EventPhase2
	EventDied
)

// Boss is the hollow warden's health, poise and attack state machine.
type Boss struct {
	Health, Poise float64
	State         BossState
	Attack        Attack
	Phase         int

	until float64
	combo []Attack
	rng   *rand.Rand
}

// NewBoss returns a boss at full health approaching its target; seed fixes its attack choices.
func NewBoss(seed uint64) *Boss {
	return &Boss{Health: bossMaxHealth, Poise: bossMaxPoise, Phase: 1, rng: rand.New(rand.NewPCG(seed, seed^0x9e3779b97f4a7c15))}
}

// HealthFraction is the boss bar's fill.
func (b *Boss) HealthFraction() float64 { return clamp01(b.Health / bossMaxHealth) }

// Damage is what an attack deals in the current phase.
func (b *Boss) Damage(a Attack) float64 {
	if b.Phase == 2 {
		return attackDamage[a] * phase2Damage
	}
	return attackDamage[a]
}

func (b *Boss) scale(d float64) float64 {
	if b.Phase == 2 {
		return d * phase2Speed
	}
	return d
}

// Step advances the state machine to now, with dist the horizontal distance to the target.
func (b *Boss) Step(now, dist float64) []BossEvent {
	if b.State == BossDead || now < b.until {
		return nil
	}
	switch b.State {
	case BossApproach:
		if len(b.combo) > 0 {
			return b.windup(b.popCombo(), now)
		}
		if a, ok := b.choose(dist); ok {
			return b.windup(a, now)
		}
	case BossWindup:
		b.State, b.until = BossStrike, now+timings[b.Attack].active
		return []BossEvent{EventStrike}
	case BossStrike:
		if len(b.combo) > 0 {
			return b.windup(b.popCombo(), now)
		}
		b.State, b.until = BossRecover, now+b.scale(timings[b.Attack].recover)
	case BossRecover, BossStagger, BossRoar:
		b.State = BossApproach
	}
	return nil
}

func (b *Boss) windup(a Attack, now float64) []BossEvent {
	b.State, b.Attack, b.until = BossWindup, a, now+b.scale(timings[a].windup)
	return []BossEvent{EventWindup}
}

func (b *Boss) popCombo() Attack {
	a := b.combo[0]
	b.combo = b.combo[1:]
	return a
}

// choose picks an attack in range, or false to keep approaching.
func (b *Boss) choose(dist float64) (Attack, bool) {
	switch {
	case dist <= meleeRange:
		if b.Phase == 2 && b.rng.IntN(3) == 0 {
			b.combo = []Attack{AttackSwipe}
			return AttackSlam, true
		}
		if b.rng.IntN(2) == 0 {
			return AttackSlam, true
		}
		return AttackSwipe, true
	case dist <= lungeRange && b.rng.IntN(3) > 0:
		return AttackLunge, true
	}
	return 0, false
}

// Moving reports whether the boss walks toward its target this tick.
func (b *Boss) Moving() bool { return b.State == BossApproach }

// Lunging reports whether the boss is dashing.
func (b *Boss) Lunging() bool { return b.State == BossStrike && b.Attack == AttackLunge }

// Hurt applies damage and poise damage; enough poise damage staggers, half health starts phase 2.
func (b *Boss) Hurt(dmg, poise, now float64) []BossEvent {
	if b.State == BossDead || dmg <= 0 {
		return nil
	}
	b.Health = max(0, b.Health-dmg)
	if b.Health <= 0 {
		b.State, b.combo = BossDead, nil
		return []BossEvent{EventDied}
	}
	if b.Phase == 1 && b.Health <= bossMaxHealth*phase2Threshold {
		b.Phase, b.Poise, b.combo = 2, bossMaxPoise, nil
		b.State, b.until = BossRoar, now+roarTime
		return []BossEvent{EventPhase2}
	}
	if b.State == BossRoar {
		return nil
	}
	b.Poise -= poise
	if b.Poise <= 0 {
		b.Poise, b.combo = bossMaxPoise, nil
		b.State, b.until = BossStagger, now+staggerTime
		return []BossEvent{EventStagger}
	}
	return nil
}
