package showcase

// deathScreenTime is how long "YOU DIED" holds before the player returns to grace.
const deathScreenTime = 5.0

// FlowState is where the fight stands for the players in the arena.
type FlowState int

const (
	FlowFighting FlowState = iota
	FlowDying
	FlowWon
)

// Flow sequences death, respawn and victory around the fight.
type Flow struct {
	State FlowState
	until float64
}

// PlayerDied starts the death screen; it reports false if the player was already dying or the fight is won.
func (f *Flow) PlayerDied(now float64) bool {
	if f.State != FlowFighting {
		return false
	}
	f.State, f.until = FlowDying, now+deathScreenTime
	return true
}

// BossDied ends the fight in victory unless the player died first.
func (f *Flow) BossDied() bool {
	if f.State != FlowFighting {
		return false
	}
	f.State = FlowWon
	return true
}

// Tick reports whether the death screen has finished and the fight must reset at grace.
func (f *Flow) Tick(now float64) bool {
	if f.State == FlowDying && now >= f.until {
		f.State = FlowFighting
		return true
	}
	return false
}

// Reset restarts the fight, as resting at grace or /showcase reset do.
func (f *Flow) Reset() { *f = Flow{} }
