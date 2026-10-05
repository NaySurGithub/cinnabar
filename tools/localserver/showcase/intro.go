package showcase

import (
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/cinema"
)

const (
	// introWait is how long a fresh arena waits for a client part before fighting without the intro.
	introWait    = 45.0
	screenWidth  = 28
	screenHeight = 15.75
)

type introState int

const (
	introNone introState = iota
	introWaiting
	introPlaying
)

// intro holds the boss back while the cinema plays on the arena screen.
type intro struct {
	cin   *cinema.Cinema
	state introState
	since float64
	run   int // tells a finished play's callback whether it is still the current one
}

// SetIntro plays cin's video on the arena screen before each new arena's fight; nil disables it.
func (c *Controller) SetIntro(cin *cinema.Cinema) { c.intro.cin = cin }

// IntroScreen stands against the inside of the north wall, facing the arena, low enough to
// fit a 16:9 view from the player's spawn.
func (a Arena) IntroScreen() cinema.Screen {
	return cinema.Screen{
		Pos: mgl64.Vec3{
			float64(a.Origin.X()) + 0.5,
			float64(a.floorY()+2) + screenHeight/2,
			float64(a.Origin.Z()-arenaHalf) + 1.05,
		},
		Width:  screenWidth,
		Height: screenHeight,
		Yaw:    0,
	}
}

// beginIntro defers the fight until the intro is over; it reports false without a cinema.
func (c *Controller) beginIntro(tx *world.Tx) bool {
	if c.intro.cin == nil {
		return false
	}
	c.stopFight(tx)
	c.intro.state, c.intro.since = introWaiting, c.now
	return true
}

// endIntro skips a running intro, as resetting or disabling the showcase do.
func (c *Controller) endIntro(players []*player.Player) {
	if c.intro.state == introPlaying {
		for _, p := range players {
			c.intro.cin.Skip(p.UUID())
		}
	}
	c.intro.state = introNone
	c.intro.run++
}

// tickIntro starts the video once a player's client part is active and reports whether the
// intro still holds the fight back.
func (c *Controller) tickIntro(players []*player.Player) bool {
	switch c.intro.state {
	case introPlaying:
		return true
	case introWaiting:
		for _, p := range players {
			if c.intro.cin.Ready(p.UUID()) {
				c.playIntro(p)
				return true
			}
		}
		if c.now-c.intro.since < introWait {
			return true
		}
		c.log.Info("showcase intro skipped: no client part")
		c.intro.state = introNone
	}
	return false
}

func (c *Controller) playIntro(p *player.Player) {
	c.intro.state = introPlaying
	c.intro.run++
	run, w := c.intro.run, c.w
	c.intro.cin.Play(p.UUID(), c.arena.IntroScreen(), func(outcome cinema.Outcome) {
		w.Do(func(tx *world.Tx) {
			if c.intro.state != introPlaying || c.intro.run != run {
				return
			}
			c.log.Info("showcase intro over", "outcome", outcome)
			c.intro.state = introNone
			if c.arena != nil {
				c.startFight(tx)
			}
		})
	})
}
