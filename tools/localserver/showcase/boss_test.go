package showcase

import (
	"math"
	"slices"
	"testing"
)

// stepUntil steps the boss every tick from now and returns the time the first event fires.
func stepUntil(b *Boss, now, dist float64, want BossEvent) float64 {
	for range 400 {
		now += dt
		if slices.Contains(b.Step(now, dist), want) {
			return now
		}
	}
	return math.Inf(1)
}

func near(a, b float64) bool { return math.Abs(a-b) <= dt+1e-9 }

func TestBossApproachesUntilTheTargetIsInRange(t *testing.T) {
	b := NewBoss(1)
	for now := dt; now < 5; now += dt {
		if ev := b.Step(now, lungeRange+5); len(ev) > 0 || !b.Moving() {
			t.Fatalf("boss attacked a distant target: events %v state %v", ev, b.State)
		}
	}
}

func TestBossTelegraphsBeforeEveryStrikeWithTheAttacksWindup(t *testing.T) {
	b := NewBoss(2)
	now := 0.0
	for range 6 {
		windup := stepUntil(b, now, 2, EventWindup)
		strike := stepUntil(b, windup, 2, EventStrike)
		if !near(strike-windup, timings[b.Attack].windup) {
			t.Fatalf("attack %v struck %.2fs after its telegraph, want %.2fs", b.Attack, strike-windup, timings[b.Attack].windup)
		}
		now = strike
	}
}

func TestBossRecoversAfterAStrikeBeforeAttackingAgain(t *testing.T) {
	b := NewBoss(3)
	strike := stepUntil(b, stepUntil(b, 0, 2, EventWindup), 2, EventStrike)
	attack := b.Attack
	next := stepUntil(b, strike, 2, EventWindup)
	if gap := next - strike; gap < timings[attack].active+timings[attack].recover-dt {
		t.Fatalf("next telegraph %.2fs after a %v strike, want at least active+recovery", gap, attack)
	}
}

func TestPhaseTwoStartsAtHalfHealthWithACueAndFasterAttacks(t *testing.T) {
	b := NewBoss(4)
	if ev := b.Hurt(bossMaxHealth*0.49, 0, 0); len(ev) != 0 || b.Phase != 1 {
		t.Fatalf("phase changed above half health: %v phase %d", ev, b.Phase)
	}
	if ev := b.Hurt(bossMaxHealth*0.02, 0, 1); !slices.Equal(ev, []BossEvent{EventPhase2}) || b.Phase != 2 {
		t.Fatalf("no phase 2 cue at half health: %v phase %d", ev, b.Phase)
	}
	if b.State != BossRoar {
		t.Fatalf("state = %v, want roar", b.State)
	}
	windup := stepUntil(b, 1, 2, EventWindup)
	if windup < 1+roarTime-dt {
		t.Fatalf("boss attacked %.2fs into its %.1fs roar", windup-1, roarTime)
	}
	strike := stepUntil(b, windup, 2, EventStrike)
	if want := timings[b.Attack].windup * phase2Speed; !near(strike-windup, want) {
		t.Fatalf("phase 2 wind-up %.2fs, want %.2fs", strike-windup, want)
	}
	if b.Damage(AttackSlam) <= attackDamage[AttackSlam] {
		t.Fatal("phase 2 does not hit harder")
	}
}

func TestPhaseTwoAddsASlamSwipeCombo(t *testing.T) {
	b := NewBoss(5)
	b.Hurt(bossMaxHealth*0.6, 0, 0)
	now := roarTime
	for range 60 {
		w := stepUntil(b, now, 2, EventWindup)
		first := b.Attack
		s := stepUntil(b, w, 2, EventStrike)
		ev := b.Step(s+timings[first].active+dt/2, 2)
		if first == AttackSlam && slices.Contains(ev, EventWindup) && b.Attack == AttackSwipe {
			return
		}
		now = s
	}
	t.Fatal("phase 2 never chained a slam into a swipe")
}

func TestPoiseBreakStaggersAndInterruptsTheWindup(t *testing.T) {
	b := NewBoss(6)
	stepUntil(b, 0, 2, EventWindup)
	if ev := b.Hurt(5, bossMaxPoise/2, 1); len(ev) != 0 {
		t.Fatalf("half poise damage staggered: %v", ev)
	}
	if ev := b.Hurt(5, bossMaxPoise/2, 1); !slices.Equal(ev, []BossEvent{EventStagger}) || b.State != BossStagger {
		t.Fatalf("poise break: events %v state %v", ev, b.State)
	}
	if ev := b.Step(1+staggerTime-dt, 2); len(ev) != 0 {
		t.Fatalf("staggered boss acted: %v", ev)
	}
}

func TestBossDiesOnceAtZeroHealth(t *testing.T) {
	b := NewBoss(7)
	b.Phase = 2
	if ev := b.Hurt(bossMaxHealth*2, 0, 0); !slices.Equal(ev, []BossEvent{EventDied}) || b.State != BossDead {
		t.Fatalf("lethal hit: %v state %v", ev, b.State)
	}
	if ev := b.Hurt(10, 10, 1); ev != nil {
		t.Fatalf("dead boss reacted: %v", ev)
	}
	if ev := b.Step(5, 1); ev != nil {
		t.Fatalf("dead boss acted: %v", ev)
	}
}
