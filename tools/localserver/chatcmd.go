package main

import (
	"errors"
	"math"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/protocol"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/cinema"
)

const (
	minSpeedMultiplier = 0.1
	maxSpeedMultiplier = 100
	// Dragonfly's sprint scales the movement attribute by this on start and divides it back on stop.
	sprintSpeedFactor = 1.3
	// Steady vanilla speeds per unit of FlySpeed and movement attribute, from the client's flight
	// and walking physics: 0.05 flies 10.9 blocks/s, 0.1 walks 4.32 blocks/s.
	flyBlocksPerSecondPerUnit  = 217.8
	walkBlocksPerSecondPerUnit = 43.17
)

// registerChatCommands registers the testing commands; every player may run them.
func registerChatCommands() {
	cmd.Register(cmd.New("speed", "Scales flight and walking speed from the vanilla defaults", nil, speedReset{}, speedSet{}))
	cmd.Register(cmd.New("fly", "Toggles being allowed to fly", nil, flyToggle{}))
	cmd.Register(cmd.New("tp", "Teleports to a position", []string{"teleport"}, teleport{}))
	cmd.Register(cmd.New("intro", "Plays the client part intro video on a screen ahead, or stops it", nil, introPlay{}, introStop{}))
}

// speedMultiplier clamps m to the accepted range.
func speedMultiplier(m float64) (float64, error) {
	if math.IsNaN(m) {
		return 0, errors.New("speed multiplier must be a number")
	}
	return min(max(m, minSpeedMultiplier), maxSpeedMultiplier), nil
}

// applySpeed sends the abilities fly speeds and the movement attribute scaled by m.
func applySpeed(p *player.Player, m float64) {
	p.SetFlightSpeed(protocol.AbilityBaseFlySpeed * m)
	p.SetVerticalFlightSpeed(protocol.AbilityBaseVerticalFlySpeed * m)
	walk := protocol.AbilityBaseWalkSpeed * m
	if p.Sprinting() {
		walk *= sprintSpeedFactor
	}
	p.SetSpeed(walk)
}

func reportSpeed(o *cmd.Output, m float64) {
	fly := protocol.AbilityBaseFlySpeed * m * flyBlocksPerSecondPerUnit
	o.Printf("Speed x%g: flying ~%.0f blocks/s (sprinting ~%.0f), walking ~%.1f blocks/s", m, fly, 2*fly, protocol.AbilityBaseWalkSpeed*m*walkBlocksPerSecondPerUnit)
}

type speedSet struct {
	Multiplier cmd.Optional[float64] `cmd:"multiplier"`
}

func (s speedSet) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can change their speed")
		return
	}
	m, err := speedMultiplier(s.Multiplier.LoadOr(1))
	if err != nil {
		o.Error(err)
		return
	}
	applySpeed(p, m)
	reportSpeed(o, m)
}

type speedReset struct {
	Reset cmd.SubCommand `cmd:"reset"`
}

func (speedReset) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	speedSet{}.Run(src, o, tx)
}

// flightMode lets a game mode without flight fly. Player data cannot store it and saves survival.
type flightMode struct{ world.GameMode }

func (flightMode) AllowsFlying() bool { return true }

type flyToggle struct{}

func (flyToggle) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can fly")
		return
	}
	switch mode := p.GameMode().(type) {
	case flightMode:
		p.SetGameMode(mode.GameMode)
		o.Print("Flight disabled")
	default:
		if mode.AllowsFlying() {
			o.Print("This game mode already allows flight")
			return
		}
		p.SetGameMode(flightMode{mode})
		o.Print("Flight enabled: double-tap jump to fly")
	}
}

type teleport struct {
	Destination mgl64.Vec3 `cmd:"destination"`
}

func (t teleport) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can teleport")
		return
	}
	p.Teleport(t.Destination)
	o.Printf("Teleported to %.1f, %.1f, %.1f", t.Destination[0], t.Destination[1], t.Destination[2])
}

// The /intro screen: centre ahead of and above the caller, 16:9, facing back at them.
const (
	introDistance = 12
	introRise     = 6
	introWidth    = 24
	introHeight   = 13.5
)

type introPlay struct{}

func (introPlay) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can watch the intro")
		return
	}
	yaw := p.Rotation().Yaw()
	rad := mgl64.DegToRad(yaw)
	ahead := mgl64.Vec3{-math.Sin(rad), 0, math.Cos(rad)}
	screen := cinema.Screen{
		Pos:    p.Position().Add(ahead.Mul(introDistance)).Add(mgl64.Vec3{0, introRise, 0}),
		Width:  introWidth,
		Height: introHeight,
		Yaw:    yaw + 180,
	}
	handle := p.H()
	cinema.Play(p.UUID(), screen, func(outcome cinema.Outcome) {
		handle.Do(func(_ *world.Tx, e world.Entity) {
			if p, ok := e.(*player.Player); ok {
				p.Messagef("Intro over: %s", outcome)
			}
		})
	})
	o.Print("Intro started")
}

type introStop struct {
	Stop cmd.SubCommand `cmd:"stop"`
}

func (introStop) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can stop the intro")
		return
	}
	cinema.Skip(p.UUID())
	o.Print("Intro stopped")
}
