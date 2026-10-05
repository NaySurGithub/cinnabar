package showcase

import (
	"strconv"
	"strings"
	"testing"
)

// field decodes field k the way the pack's bindings do: the first 6+3k bytes with the first
// 3+3k removed, read as an integer, minus the offset.
func field(t *testing.T, s string, k int) int {
	t.Helper()
	n, err := strconv.Atoi(strings.Replace(s[:6+3*k], s[:3+3*k], "", 1))
	if err != nil {
		t.Fatalf("field %d of %q: %v", k, s, err)
	}
	return n - 100
}

func TestHUDEncodesFixedWidthFieldsThePackCanSlice(t *testing.T) {
	h := HUD{Stamina: 75, Health: 90, Energy: 40, FlashCooldown: 0, MeteorCooldown: 50, Active: AbilityCharge}
	s := h.Encode()
	if s != "cnb175190140100150101" {
		t.Fatalf("Encode() = %q", s)
	}
	for k, want := range []int{75, 90, 40, 0, 50, int(AbilityCharge)} {
		if got := field(t, s, k); got != want {
			t.Errorf("field %d = %d, want %d", k, got, want)
		}
	}
}

func TestHUDClampsOutOfRangeValues(t *testing.T) {
	s := HUD{Stamina: -5, Health: 250}.Encode()
	if len(s) != len(hudPrefix)+6*3 || field(t, s, 0) != 0 || field(t, s, 1) != 100 {
		t.Fatalf("Encode() = %q", s)
	}
}

func TestHUDShowsANearlyFinishedCooldownAsNotReady(t *testing.T) {
	f := NewFighter()
	if err := f.Flash(0); err != nil {
		t.Fatal(err)
	}
	if h := hudOf(f, 20, 20, flashCooldown-0.01); h.FlashCooldown == 0 {
		t.Fatal("cooldown showed ready before it was")
	}
	if h := hudOf(f, 20, 20, flashCooldown); h.FlashCooldown != 0 {
		t.Fatalf("cooldown = %d when ready", h.FlashCooldown)
	}
}
