// Package cinema plays the showcase intro video on a client part's in-world screen and reports
// when it is over, so a fight can start. docs/experience-runtime.md describes the developer setup.
package cinema

import (
	"math"
	"sync"
	"time"

	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// The client part contract shared with the showcase-screen guest.
const (
	BundleID       = "showcase"
	ScreenChannel  = "showcase.screen"
	EventChannel   = "showcase.media"
	Schema         = 1
	DescriptorPath = "media/intro.json"
)

// TimeoutSlack is how long past the video's duration a play waits for its end before giving up.
const TimeoutSlack = 30 * time.Second

// Screen actions, as the `showcase.screen` choice field numbers them.
const (
	actionPlay uint16 = iota
	actionPause
	actionStop
)

// Client events, as the `showcase.media` choice field numbers them.
const (
	eventPlaying uint16 = iota
	eventPaused
	eventStopped
	eventEnded
)

// Screen places the video: Pos is its centre, Width and Height are in blocks, and its visible face
// looks along Minecraft yaw Yaw in degrees (0 toward +Z).
type Screen struct {
	Pos           mgl64.Vec3
	Width, Height float64
	Yaw           float64
}

// Outcome is why a play is over.
type Outcome int

const (
	// Ended means the client reported the end of the video.
	Ended Outcome = iota
	// Stopped means the client stopped playback, for example after a decode failure or F9.
	Stopped
	// Skipped means the server skipped the video.
	Skipped
	// Fallback means the player has no active client part, so nothing played.
	Fallback
	// Timeout means the client never reported an end within the duration and TimeoutSlack.
	Timeout
)

func (o Outcome) String() string {
	return [...]string{"ended", "stopped", "skipped", "fallback", "timeout"}[o]
}

// Sender carries a record to a player's client part; extension.Server implements it.
type Sender interface {
	Send(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) bool
}

// Cinema tracks one intro play per player. Its methods may be called from any goroutine; done
// callbacks run without its lock held, exactly once per Play.
type Cinema struct {
	send    Sender
	timeout time.Duration
	after   func(time.Duration, func()) func() bool

	mu    sync.Mutex
	plays map[uuid.UUID]*play
}

type play struct {
	done func(Outcome)
	stop func() bool
	// staleStops counts stop events still due from the play this one replaced.
	staleStops int
	playing    bool
}

// New returns a Cinema that times a play out after duration plus TimeoutSlack.
func New(send Sender, duration time.Duration) *Cinema {
	return &Cinema{
		send:    send,
		timeout: duration + TimeoutSlack,
		after: func(d time.Duration, f func()) func() bool {
			return time.AfterFunc(d, f).Stop
		},
		plays: make(map[uuid.UUID]*play),
	}
}

// Play shows the intro on screen for player and calls done once it is over. A play already running
// for player is skipped first.
func (c *Cinema) Play(player uuid.UUID, screen Screen, done func(Outcome)) {
	replaced := c.Skip(player)
	if !c.send.Send(player, BundleID, ScreenChannel, Schema, screenRecord(screen, actionPlay)) {
		done(Fallback)
		return
	}
	p := &play{done: done}
	if replaced {
		p.staleStops = 1
	}
	c.mu.Lock()
	c.plays[player] = p
	p.stop = c.after(c.timeout, func() { c.finish(player, p, Timeout) })
	c.mu.Unlock()
}

// Skip stops player's play, if any, finishes it as Skipped and reports whether there was one.
func (c *Cinema) Skip(player uuid.UUID) bool {
	c.mu.Lock()
	p := c.plays[player]
	c.mu.Unlock()
	if p == nil {
		return false
	}
	c.send.Send(player, BundleID, ScreenChannel, Schema, screenRecord(Screen{Width: 1, Height: 1}, actionStop))
	c.finish(player, p, Skipped)
	return true
}

// Receive handles a `showcase.media` event from player's client part; it reports whether the
// message belonged to the cinema.
func (c *Cinema) Receive(player uuid.UUID, channel string, schema uint16, payload []experience.Scalar) bool {
	if channel != EventChannel || schema != Schema || len(payload) != 2 || payload[0].Choice == nil {
		return false
	}
	var outcome Outcome
	switch *payload[0].Choice {
	case eventPlaying:
		// Time the play from its first frame, so slow startup or rebuffering is not a timeout.
		c.mu.Lock()
		if p := c.plays[player]; p != nil {
			p.playing = true
			if p.stop != nil {
				p.stop()
			}
			p.stop = c.after(c.timeout, func() { c.finish(player, p, Timeout) })
		}
		c.mu.Unlock()
		return true
	case eventEnded:
		outcome = Ended
	case eventStopped:
		outcome = Stopped
	default:
		return true
	}
	c.mu.Lock()
	p := c.plays[player]
	if p != nil && outcome == Stopped && !p.playing && p.staleStops > 0 {
		p.staleStops--
		p = nil
	}
	c.mu.Unlock()
	if p != nil {
		c.finish(player, p, outcome)
	}
	return true
}

// Ready reports whether player's client part is active, so a Play would not fall back at once.
func (c *Cinema) Ready(player uuid.UUID) bool {
	active, ok := c.send.(interface{ Active(uuid.UUID) bool })
	return !ok || active.Active(player)
}

// finish ends p with outcome unless it already ended.
func (c *Cinema) finish(player uuid.UUID, p *play, outcome Outcome) {
	c.mu.Lock()
	if c.plays[player] != p {
		c.mu.Unlock()
		return
	}
	delete(c.plays, player)
	stop := p.stop
	c.mu.Unlock()
	if stop != nil {
		stop()
	}
	if outcome != Skipped {
		// Take the screen down; Skip has already sent its stop.
		c.send.Send(player, BundleID, ScreenChannel, Schema, screenRecord(Screen{Width: 1, Height: 1}, actionStop))
	}
	p.done(outcome)
}

// screenRecord is the `showcase.screen` record: centre in centiblocks, size in centiblocks, yaw in
// whole degrees and the action.
func screenRecord(s Screen, action uint16) []experience.Scalar {
	cm := func(blocks float64) experience.Scalar {
		v := int64(math.Round(blocks * 100))
		return experience.Scalar{Integer: &v}
	}
	size := func(blocks float64) experience.Scalar {
		v := min(max(int64(math.Round(blocks*100)), 1), 6400)
		return experience.Scalar{Integer: &v}
	}
	yaw := int64(math.Round(math.Mod(s.Yaw, 360)))
	return []experience.Scalar{
		cm(s.Pos[0]), cm(s.Pos[1]), cm(s.Pos[2]),
		size(s.Width), size(s.Height),
		{Integer: &yaw},
		{Choice: &action},
	}
}

var (
	defaultMu sync.Mutex
	current   *Cinema
)

// SetDefault installs the Cinema that the package-level Play and Skip use; nil removes it.
func SetDefault(c *Cinema) {
	defaultMu.Lock()
	defer defaultMu.Unlock()
	current = c
}

// Play plays the intro with the default Cinema, or finishes at once with Fallback without one.
func Play(player uuid.UUID, screen Screen, done func(Outcome)) {
	defaultMu.Lock()
	c := current
	defaultMu.Unlock()
	if c == nil {
		done(Fallback)
		return
	}
	c.Play(player, screen, done)
}

// Skip skips player's play on the default Cinema.
func Skip(player uuid.UUID) {
	defaultMu.Lock()
	c := current
	defaultMu.Unlock()
	if c != nil {
		c.Skip(player)
	}
}
