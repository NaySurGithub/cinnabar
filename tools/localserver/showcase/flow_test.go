package showcase

import "testing"

func TestDeathHoldsTheDeathScreenThenResetsAtGrace(t *testing.T) {
	var f Flow
	if !f.PlayerDied(10) || f.State != FlowDying {
		t.Fatalf("death not registered: %v", f.State)
	}
	if f.PlayerDied(11) {
		t.Fatal("a second death restarted the death screen")
	}
	if f.BossDied() {
		t.Fatal("the boss was felled while the player lay dead")
	}
	if f.Tick(10 + deathScreenTime - dt) {
		t.Fatal("reset before the death screen finished")
	}
	if !f.Tick(10+deathScreenTime) || f.State != FlowFighting {
		t.Fatalf("no reset after the death screen: %v", f.State)
	}
}

func TestVictoryIsFinalUntilReset(t *testing.T) {
	var f Flow
	if !f.BossDied() || f.State != FlowWon {
		t.Fatalf("victory not registered: %v", f.State)
	}
	if f.PlayerDied(1) || f.Tick(100) {
		t.Fatal("a won fight still ran the death flow")
	}
	f.Reset()
	if f.State != FlowFighting {
		t.Fatalf("reset left %v", f.State)
	}
}
