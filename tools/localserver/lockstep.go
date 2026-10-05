package main

import (
	"log/slog"
	"sync/atomic"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/showcase"
)

// lockstep ticks the overworld once per PlayerAuthInput of the newest connection. A client on a
// fixed recording clock sends one per 1/20 s of game time however slowly it renders, so server
// time follows the client's game time exactly.
type lockstep struct {
	ticks chan struct{}
	clock atomic.Pointer[stepConn]
}

func newLockstep() *lockstep { return &lockstep{ticks: make(chan struct{}, 256)} }

// Listener wraps a listener constructor so its connections report their input packets.
func (l *lockstep) Listener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		ln, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &stepListener{Listener: ln, l: l}, nil
	}
}

// run applies queued ticks until the world closes: a world tick, then the showcase's.
func (l *lockstep) run(w *world.World, show *showcase.Controller, log *slog.Logger) {
	n := 0
	for range l.ticks {
		if n++; n%400 == 0 {
			log.Info("lockstep ticks", "ticks", n)
		}
		w.AdvanceTick()
		if err := show.Step().Err(); err != nil {
			return
		}
	}
}

type stepListener struct {
	server.Listener
	l *lockstep
}

func (s *stepListener) Accept() (session.Conn, error) {
	c, err := s.Listener.Accept()
	if err != nil {
		return nil, err
	}
	wrapped := &stepConn{Conn: c, l: s.l}
	s.l.clock.Store(wrapped)
	return wrapped, nil
}

// Disconnect hands the inner listener the connection it accepted.
func (s *stepListener) Disconnect(c session.Conn, reason string) error {
	if wrapped, ok := c.(*stepConn); ok {
		c = wrapped.Conn
	}
	return s.Listener.Disconnect(c, reason)
}

type stepConn struct {
	session.Conn
	l *lockstep
}

func (c *stepConn) ReadPacket() (packet.Packet, error) {
	pk, err := c.Conn.ReadPacket()
	if _, ok := pk.(*packet.PlayerAuthInput); ok && c.l.clock.Load() == c {
		select {
		case c.l.ticks <- struct{}{}:
		default: // a stalled world drops ticks rather than blocking the connection
		}
	}
	return pk, err
}
