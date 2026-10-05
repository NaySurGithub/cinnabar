package showcase

import (
	"errors"
	"math"
	"testing"
)

func tickFor(f *Fighter, from, seconds float64, flying bool) float64 {
	now := from
	for range int(math.Round(seconds / dt)) {
		now += dt
		f.Tick(now, dt, flying)
	}
	return now
}

func TestStaminaDrainsPerActionAndRegeneratesOnlyAfterIdleDelay(t *testing.T) {
	f := NewFighter()
	if err := f.Attack(false, 0); err != nil {
		t.Fatal(err)
	}
	if want := maxStamina - lightCost; f.Stamina != want {
		t.Fatalf("after light attack stamina = %v, want %v", f.Stamina, want)
	}
	if err := f.Attack(true, 1); err != nil {
		t.Fatal(err)
	}
	spent := f.Stamina
	now := tickFor(f, 1, staminaRegenDelay-2*dt, false)
	if f.Stamina != spent {
		t.Fatalf("stamina regenerated during the idle delay: %v -> %v", spent, f.Stamina)
	}
	tickFor(f, now, 3, false)
	if f.Stamina != maxStamina {
		t.Fatalf("stamina = %v after resting, want full", f.Stamina)
	}
}

func TestHeavyAttackRecoversLongerThanLight(t *testing.T) {
	f := NewFighter()
	if err := f.Attack(false, 0); err != nil {
		t.Fatal(err)
	}
	if err := f.Attack(false, lightRecovery); err != nil {
		t.Fatalf("light attack not ready after its recovery: %v", err)
	}
	if err := f.Attack(true, 2); err != nil {
		t.Fatal(err)
	}
	if err := f.Attack(false, 2+lightRecovery); !errors.Is(err, errRecovering) {
		t.Fatalf("attack during heavy recovery = %v, want errRecovering", err)
	}
	if err := f.Attack(false, 2+heavyRecovery); err != nil {
		t.Fatalf("attack after heavy recovery: %v", err)
	}
}

func TestDodgeNeedsStaminaAndGrantsIFramesForItsWindow(t *testing.T) {
	f := NewFighter()
	f.Stamina = 0
	if err := f.Dodge(0); !errors.Is(err, errNoStamina) {
		t.Fatalf("dodge without stamina = %v, want errNoStamina", err)
	}
	f.Stamina = 50
	if err := f.Dodge(10); err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		at   float64
		want bool
	}{{10, true}, {10 + dodgeIFrames - 0.01, true}, {10 + dodgeIFrames, false}} {
		if got := f.Invulnerable(c.at); got != c.want {
			t.Errorf("Invulnerable(%v) = %v, want %v", c.at, got, c.want)
		}
	}
	if err := f.Dodge(10 + dodgeCooldown/2); !errors.Is(err, errCooldown) {
		t.Fatalf("spammed dodge = %v, want errCooldown", err)
	}
}

func TestSneakDoubleTapWithinWindowDodgesOnce(t *testing.T) {
	f := NewFighter()
	if f.SneakDown(1) {
		t.Fatal("first press reported a double tap")
	}
	if !f.SneakDown(1 + doubleTapWindow - 0.05) {
		t.Fatal("second press inside the window was not a double tap")
	}
	if f.SneakDown(1 + doubleTapWindow) {
		t.Fatal("a third press reused the consumed tap")
	}
	if f.SneakDown(5) {
		t.Fatal("a press long after the last was a double tap")
	}
}

func TestChargeFillsEnergyAndEndsAtItsLimit(t *testing.T) {
	f := NewFighter()
	f.Energy = 0
	if err := f.StartCharge(0); err != nil {
		t.Fatal(err)
	}
	tickFor(f, 0, 1, false)
	if math.Abs(f.Energy-chargeRate) > 1e-6 {
		t.Fatalf("energy after 1 s of charge = %v, want %v", f.Energy, chargeRate)
	}
	if f.Active() != AbilityCharge {
		t.Fatalf("active = %v, want charge", f.Active())
	}
	ended := AbilityNone
	for now := 1.0; now <= chargeMax+dt; now += dt {
		if a := f.Tick(now, dt, false); a != AbilityNone {
			ended = a
		}
	}
	if ended != AbilityCharge || f.Active() != AbilityNone {
		t.Fatalf("charge did not time out: ended=%v active=%v", ended, f.Active())
	}
}

func TestBeamRequiresEnergyAndStopsWhenDrained(t *testing.T) {
	f := NewFighter()
	f.Energy = beamMinEnergy - 1
	if err := f.StartBeam(0); !errors.Is(err, errNoEnergy) {
		t.Fatalf("beam on low energy = %v, want errNoEnergy", err)
	}
	f.Energy = 20
	if err := f.StartBeam(0); err != nil {
		t.Fatal(err)
	}
	if err := f.StartCharge(0); !errors.Is(err, errBusy) {
		t.Fatalf("charge while beaming = %v, want errBusy", err)
	}
	tickFor(f, 0, 20/beamDrain+dt, false)
	if f.Energy != 0 || f.Active() != AbilityNone {
		t.Fatalf("beam still running at energy %v (active %v)", f.Energy, f.Active())
	}
}

func TestFlashStepSpendsEnergyGrantsIFramesAndCoolsDown(t *testing.T) {
	f := NewFighter()
	if err := f.Flash(3); err != nil {
		t.Fatal(err)
	}
	if f.Energy != startingEnergy-flashCost || !f.Invulnerable(3+flashIFrames/2) {
		t.Fatalf("flash: energy %v, invulnerable %v", f.Energy, f.Invulnerable(3+flashIFrames/2))
	}
	if err := f.Flash(3 + flashCooldown/2); !errors.Is(err, errCooldown) {
		t.Fatalf("flash during cooldown = %v, want errCooldown", err)
	}
	if got := f.Cooldown(AbilityFlash, 3+flashCooldown/2); math.Abs(got-0.5) > 1e-9 {
		t.Fatalf("cooldown fraction halfway = %v, want 0.5", got)
	}
	f.Energy = flashCost - 1
	if err := f.Flash(3 + flashCooldown); !errors.Is(err, errNoEnergy) {
		t.Fatalf("flash without energy = %v, want errNoEnergy", err)
	}
}

func TestMeteorLandsOnlyAfterLeavingTheGround(t *testing.T) {
	f := NewFighter()
	if err := f.Meteor(0); err != nil {
		t.Fatal(err)
	}
	if f.Land(true, meteorMinAir/2) {
		t.Fatal("meteor crashed on the take-off tick")
	}
	if f.Land(false, meteorMinAir+0.1) {
		t.Fatal("meteor crashed in the air")
	}
	if !f.Land(true, meteorMinAir+0.2) {
		t.Fatal("meteor did not crash on landing")
	}
	if err := f.Meteor(1); !errors.Is(err, errCooldown) {
		t.Fatalf("meteor during cooldown = %v, want errCooldown", err)
	}
}

func TestFlightDrainsEnergyUntilItIsRevoked(t *testing.T) {
	f := NewFighter()
	f.Energy = flightDrain
	tickFor(f, 0, 0.5, true)
	if !f.CanFly() || f.Active() != AbilityFlying {
		t.Fatalf("flight revoked early: energy %v active %v", f.Energy, f.Active())
	}
	tickFor(f, 0.5, 0.6, true)
	if f.CanFly() {
		t.Fatalf("flight still granted at energy %v", f.Energy)
	}
}
