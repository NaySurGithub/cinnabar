package cinema

import (
	"sync"
	"testing"
	"time"

	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

type sent struct {
	channel string
	payload []experience.Scalar
}

type fakeSender struct {
	mu     sync.Mutex
	active bool
	sent   []sent
}

func (f *fakeSender) Send(_ uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.active || exp != BundleID || schema != Schema {
		return false
	}
	f.sent = append(f.sent, sent{channel, payload})
	return true
}

// manualTimer replaces time.AfterFunc so a test fires timeouts itself.
type manualTimer struct {
	mu    sync.Mutex
	fire  []func()
	delay time.Duration
}

func (m *manualTimer) schedule(d time.Duration, f func()) func() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.delay = d
	m.fire = append(m.fire, f)
	return func() bool { return true }
}

func testCinema(active bool) (*Cinema, *fakeSender, *manualTimer) {
	s := &fakeSender{active: active}
	c := New(s, 30*time.Second)
	m := &manualTimer{}
	c.after = m.schedule
	return c, s, m
}

func event(e uint16) []experience.Scalar {
	ms := int64(1234)
	return []experience.Scalar{{Choice: &e}, {Integer: &ms}}
}

// recorder counts outcomes, so a test sees a callback that ran twice.
type recorder struct {
	mu       sync.Mutex
	outcomes []Outcome
}

func (r *recorder) done(o Outcome) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.outcomes = append(r.outcomes, o)
}

func (r *recorder) want(t *testing.T, want ...Outcome) {
	t.Helper()
	r.mu.Lock()
	defer r.mu.Unlock()
	if len(r.outcomes) != len(want) {
		t.Fatalf("outcomes %v, want %v", r.outcomes, want)
	}
	for i := range want {
		if r.outcomes[i] != want[i] {
			t.Fatalf("outcomes %v, want %v", r.outcomes, want)
		}
	}
}

// A player without an active client part gets the fallback at once and nothing is scheduled.
func TestPlayFallsBackWithoutClientPart(t *testing.T) {
	c, _, timer := testCinema(false)
	var r recorder
	c.Play(uuid.New(), Screen{Width: 24, Height: 13.5}, r.done)
	r.want(t, Fallback)
	if len(timer.fire) != 0 {
		t.Fatal("a fallback play scheduled a timeout")
	}
}

// The screen record carries centiblocks, clamped size, whole-degree yaw and the play action.
func TestPlaySendsScreenRecord(t *testing.T) {
	c, s, timer := testCinema(true)
	c.Play(uuid.New(), Screen{Pos: mgl64.Vec3{1.5, -64, 2.254}, Width: 24, Height: 100, Yaw: 450}, func(Outcome) {})
	if len(s.sent) != 1 || s.sent[0].channel != ScreenChannel {
		t.Fatalf("sent %+v", s.sent)
	}
	p := s.sent[0].payload
	want := []int64{150, -6400, 225, 2400, 6400, 90}
	for i, v := range want {
		if p[i].Integer == nil || *p[i].Integer != v {
			t.Fatalf("field %d of %+v, want %d", i, p, v)
		}
	}
	if p[6].Choice == nil || *p[6].Choice != actionPlay {
		t.Fatalf("action %+v", p[6])
	}
	if timer.delay != 30*time.Second+TimeoutSlack {
		t.Fatalf("timeout %v", timer.delay)
	}
}

// The end event finishes the play once; later events, timeouts and skips change nothing.
func TestEndedEventFinishesOnce(t *testing.T) {
	c, _, timer := testCinema(true)
	player := uuid.New()
	var r recorder
	c.Play(player, Screen{Width: 1, Height: 1}, r.done)
	if !c.Receive(player, EventChannel, Schema, event(eventPlaying)) {
		t.Fatal("playing event not taken")
	}
	r.want(t)
	c.Receive(player, EventChannel, Schema, event(eventEnded))
	c.Receive(player, EventChannel, Schema, event(eventStopped))
	timer.fire[0]()
	c.Skip(player)
	r.want(t, Ended)
	if c.Receive(player, "showcase.other", Schema, event(eventEnded)) {
		t.Fatal("foreign channel taken")
	}
}

// Without an end event the play times out, and a skip sends stop and finishes as skipped.
func TestTimeoutAndSkip(t *testing.T) {
	c, s, timer := testCinema(true)
	player := uuid.New()
	var first, second recorder
	c.Play(player, Screen{Width: 1, Height: 1}, first.done)
	timer.fire[0]()
	first.want(t, Timeout)
	c.Play(player, Screen{Width: 1, Height: 1}, second.done)
	c.Skip(player)
	second.want(t, Skipped)
	last := s.sent[len(s.sent)-1].payload
	if *last[6].Choice != actionStop {
		t.Fatalf("skip sent %+v", last)
	}
	c.Receive(player, EventChannel, Schema, event(eventStopped))
	second.want(t, Skipped)
}

// Playing again replaces a running play, which finishes as skipped.
func TestReplayReplacesRunningPlay(t *testing.T) {
	c, _, _ := testCinema(true)
	player := uuid.New()
	var first, second recorder
	c.Play(player, Screen{Width: 1, Height: 1}, first.done)
	c.Play(player, Screen{Width: 1, Height: 1}, second.done)
	c.Receive(player, EventChannel, Schema, event(eventEnded))
	first.want(t, Skipped)
	second.want(t, Ended)
}

// The package-level Play falls back without a default Cinema.
func TestDefaultPlayFallsBack(t *testing.T) {
	SetDefault(nil)
	var r recorder
	Play(uuid.New(), Screen{}, r.done)
	r.want(t, Fallback)
}
