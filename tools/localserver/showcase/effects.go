package showcase

import (
	"time"

	"github.com/df-mc/dragonfly/server/player/title"
	"github.com/df-mc/dragonfly/server/world/sound"
)

// soundNamed plays a vanilla sound event by name.
func soundNamed(name string, volume, pitch float64) sound.Custom {
	return sound.Custom{Name: name, Volume: volume, Pitch: pitch}
}

// slowTitle uses the long fades of the death and victory cards.
func slowTitle(text string) title.Title {
	return title.New(text).
		WithFadeInDuration(1500 * time.Millisecond).
		WithDuration(3 * time.Second).
		WithFadeOutDuration(1500 * time.Millisecond)
}

func deathTitle() title.Title   { return slowTitle("§4YOU DIED") }
func victoryTitle() title.Title { return slowTitle("§6ENEMY FELLED") }

func phase2Title() title.Title {
	return title.New("").WithSubtitle("§6Varr rises again").
		WithFadeInDuration(500 * time.Millisecond).
		WithDuration(2 * time.Second).
		WithFadeOutDuration(time.Second)
}
