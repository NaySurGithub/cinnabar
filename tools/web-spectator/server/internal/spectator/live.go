package spectator

import (
	"sync"
	"time"
)

// Each match has its own lock. Slow viewers cannot stall unrelated exports.
type Live struct {
	mu        sync.Mutex
	id        string
	arena     *Arena
	frame     Frame
	active    bool
	closeInfo CloseInfo
	watchers  map[*Subscription]struct{}
}

// CloseInfo is retained only on an existing live handle for its subscribers.
// A terminal frame is available only after a validated finished export.
type CloseInfo struct {
	Reason     string `json:"reason"`
	FinalFrame *Frame `json:"finalFrame,omitempty"`
}

type Subscription struct {
	Updates chan struct{}
	Closed  chan struct{}
	live    *Live
}

func (live *Live) Snapshot(now time.Time) (Frame, bool) {
	live.mu.Lock()
	defer live.mu.Unlock()
	return live.frame, live.active && fresh(live.frame.UpdatedAt, now)
}

func (live *Live) ClosedInfo() CloseInfo {
	live.mu.Lock()
	defer live.mu.Unlock()
	return live.closeInfo
}

// WithCurrent keeps consent/freshness and each bounded HTTP write atomic with closure.
// Callers must put a short write deadline on the connection before invoking it.
func (live *Live) WithCurrent(now time.Time, write func(Frame, *Arena) error) (bool, error) {
	live.mu.Lock()
	defer live.mu.Unlock()
	if !live.active || !fresh(live.frame.UpdatedAt, now) {
		return false, nil
	}
	return true, write(live.frame, live.arena)
}

func (live *Live) Subscribe(now time.Time) *Subscription {
	live.mu.Lock()
	defer live.mu.Unlock()
	if !live.active || !fresh(live.frame.UpdatedAt, now) || len(live.watchers) >= 64 {
		return nil
	}
	sub := &Subscription{Updates: make(chan struct{}, 1), Closed: make(chan struct{}), live: live}
	live.watchers[sub] = struct{}{}
	return sub
}

func (sub *Subscription) Cancel() {
	sub.live.mu.Lock()
	defer sub.live.mu.Unlock()
	delete(sub.live.watchers, sub)
}

func (live *Live) update(frame Frame) {
	live.mu.Lock()
	defer live.mu.Unlock()
	if !live.active || frame.UpdatedAt.Before(live.frame.UpdatedAt) {
		return
	}
	live.frame = frame
	for sub := range live.watchers {
		select {
		case sub.Updates <- struct{}{}:
		default:
		}
	}
}

func (live *Live) close(info CloseInfo) {
	live.mu.Lock()
	defer live.mu.Unlock()
	live.deactivate(info)
}

func (live *Live) deactivate(info CloseInfo) {
	if !live.active {
		return
	}
	live.closeInfo = info
	live.active = false
	for sub := range live.watchers {
		close(sub.Closed)
	}
	clear(live.watchers)
}
