package showcase

import (
	"fmt"
	"math"
)

// hudPrefix starts the sidebar title the HUD pack decodes; SPEC.md documents the layout.
const hudPrefix = "cnb"

// HUD is what the pack's bars and ability slots show, each 0..100 except Active.
type HUD struct {
	Stamina, Health, Energy, FlashCooldown, MeteorCooldown int
	Active                                                 Ability
}

// Encode returns the fixed-width sidebar title: the prefix, then each field as value+100.
func (h HUD) Encode() string {
	field := func(v int) int { return 100 + min(100, max(0, v)) }
	return fmt.Sprintf("%s%d%d%d%d%d%d", hudPrefix,
		field(h.Stamina), field(h.Health), field(h.Energy),
		field(h.FlashCooldown), field(h.MeteorCooldown), field(int(h.Active)))
}

// percent rounds a 0..1 fraction up so a nearly empty bar or cooldown still shows.
func percent(fraction float64) int { return int(math.Ceil(clamp01(fraction) * 100)) }

// hudOf samples a fighter for the pack.
func hudOf(f *Fighter, health, maxHealth, now float64) HUD {
	return HUD{
		Stamina:        int(math.Round(f.Stamina)),
		Health:         percent(health / maxHealth),
		Energy:         int(math.Round(f.Energy)),
		FlashCooldown:  percent(f.Cooldown(AbilityFlash, now)),
		MeteorCooldown: percent(f.Cooldown(AbilityMeteor, now)),
		Active:         f.Active(),
	}
}
