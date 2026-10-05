package main

import (
	"testing"

	"github.com/df-mc/dragonfly/server/session"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type packetConn struct {
	session.Conn
	next packet.Packet
}

func (c *packetConn) ReadPacket() (packet.Packet, error) { return c.next, nil }

// Only the newest connection's input packets tick the world.
func TestLockstepTicksOncePerInputOfTheNewestConnection(t *testing.T) {
	l := newLockstep()
	old := &stepConn{Conn: &packetConn{next: &packet.PlayerAuthInput{}}, l: l}
	cur := &stepConn{Conn: &packetConn{next: &packet.PlayerAuthInput{}}, l: l}
	l.clock.Store(cur)
	for range 3 {
		_, _ = cur.ReadPacket()
	}
	_, _ = old.ReadPacket()
	cur.Conn.(*packetConn).next = &packet.Text{}
	_, _ = cur.ReadPacket()
	if got := len(l.ticks); got != 3 {
		t.Fatalf("queued %d ticks, want 3", got)
	}
}
