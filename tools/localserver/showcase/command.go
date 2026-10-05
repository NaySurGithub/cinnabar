package showcase

import (
	"slices"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// RegisterCommands registers /showcase and the client mod's /ability requests.
func RegisterCommands(c *Controller) {
	cmd.Register(cmd.New("showcase", "Builds the boss showcase arena here, or resets or stops it", nil,
		showcaseSouls{c: c}, showcaseReset{c: c}, showcaseOff{c: c}))
	cmd.Register(cmd.New("ability", "Uses a showcase ability", nil, abilityCmd{c: c}))
}

type showcaseSouls struct {
	c     *Controller
	Souls cmd.SubCommand `cmd:"souls"`
}

func (s showcaseSouls) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok || tx == nil {
		o.Error("only players can start the showcase")
		return
	}
	if err := s.c.Enable(tx, p); err != nil {
		o.Error(err)
		return
	}
	o.Print("§6" + BossName + " awaits.")
}

type showcaseReset struct {
	c     *Controller
	Reset cmd.SubCommand `cmd:"reset"`
}

func (s showcaseReset) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	if err := s.c.owns(tx); err != nil {
		o.Error(err)
		return
	}
	if !s.c.Enabled() {
		o.Error("the showcase is not running; use /showcase souls")
		return
	}
	s.c.Reset(tx)
}

type showcaseOff struct {
	c   *Controller
	Off cmd.SubCommand `cmd:"off"`
}

func (s showcaseOff) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	if err := s.c.owns(tx); err != nil {
		o.Error(err)
		return
	}
	if err := s.c.Disable(tx); err != nil {
		o.Error(err)
		return
	}
	o.Print("Showcase stopped; its pack is no longer offered.")
}

// abilityVerb is the /ability name parameter.
type abilityVerb string

func (abilityVerb) Type() string { return "ShowcaseAbility" }
func (abilityVerb) Options(cmd.Source) []string {
	names := make([]string, 0, len(abilityNames))
	for n := range abilityNames {
		names = append(names, n)
	}
	slices.Sort(names)
	return names
}

// abilityPhase is the optional start/stop of a held ability.
type abilityPhase string

func (abilityPhase) Type() string                { return "ShowcasePhase" }
func (abilityPhase) Options(cmd.Source) []string { return []string{"start", "stop"} }

type abilityCmd struct {
	c     *Controller
	Name  abilityVerb                `cmd:"ability"`
	Phase cmd.Optional[abilityPhase] `cmd:"phase"`
}

// Run is silent so a client mod can send it every press.
func (a abilityCmd) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok || tx == nil {
		o.Error("only players have abilities")
		return
	}
	// Refusals stay silent: the HUD shows energy and cooldowns, and chat lines would clutter play.
	_ = a.c.Request(tx, p, abilityNames[string(a.Name)], string(a.Phase.LoadOr("")))
}
