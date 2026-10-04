package experience

import (
	"encoding/hex"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// member is one network member that the flood found: its cell state and its data, if any.
type member struct {
	st      cellState
	data    []byte
	hasData bool
}

// flood returns the network around starts, the members that at finds: every one reachable
// from a start through face-adjacent members, in breadth-first order, as the guest applies its
// own rules about which faces connect. It stops at positions at reports no member for, unloaded
// ones included, as an AE2 grid does, and holds at most maxNetworkBlocks members and
// maxNetworkDataBytes of their data: a member past either bound ends it, truncated.
func flood(starts []cube.Pos, at func(cube.Pos) (member, bool)) ([]member, bool) {
	seen := make(map[cube.Pos]bool, len(starts))
	var queue []cube.Pos
	for _, pos := range starts {
		if !seen[pos] {
			seen[pos] = true
			queue = append(queue, pos)
		}
	}
	var members []member
	data := 0
	for len(queue) > 0 {
		pos := queue[0]
		queue = queue[1:]
		m, ok := at(pos)
		if !ok {
			continue
		}
		if len(members) == maxNetworkBlocks || data+len(m.data) > maxNetworkDataBytes {
			return members, true
		}
		members = append(members, m)
		data += len(m.data)
		for _, f := range cube.Faces() {
			if side := pos.Side(f); !seen[side] {
				seen[side] = true
				queue = append(queue, side)
			}
		}
	}
	return members, false
}

// network floods the network of the event's anchor in tx and adds its members that snap does
// not hold yet as cells, which the commit checks like the others. A callback anchored on a member
// of the Experience floods from the anchor; the break of a member floods from its six neighbors,
// and may find the two halves of a split. Any other callback has no network.
func (h *Host) network(tx *world.Tx, d *dispatcher, ev event, snap *snapshot) {
	starts := h.networkStarts(d, ev, snap)
	if starts == nil {
		return
	}
	r := tx.Range()
	members, truncated := flood(starts, func(pos cube.Pos) (member, bool) {
		if pos.OutOfBounds(r) {
			return member{}, false
		}
		st := h.cellState(tx, d.id, ev.dim, pos)
		if !st.owned || !st.block.t.network {
			return member{}, false
		}
		data, ok := h.store.Data(d.id, ev.dim.storeKey(pos))
		st.dataLen = uint64(len(data))
		return member{st: st, data: data, hasData: ok}, true
	})
	held := make(map[cube.Pos]bool, len(snap.cells))
	for _, st := range snap.cells {
		held[st.pos] = true
	}
	network := &Network{Blocks: make([]BlockPos, 0, len(members)), Truncated: truncated}
	for _, m := range members {
		network.Blocks = append(network.Blocks, blockPos(m.st.pos))
		if held[m.st.pos] {
			continue
		}
		cell := Cell{Pos: blockPos(m.st.pos), Loaded: true, ID: m.st.id, Owned: true, States: m.st.block.states()}
		if m.hasData {
			s := hex.EncodeToString(m.data)
			cell.Data = &s
		}
		snap.req.Snapshot = append(snap.req.Snapshot, cell)
		snap.cells = append(snap.cells, m.st)
	}
	snap.req.Network = network
}

// networkStarts is where the event's network flood starts: the anchor when it holds a member of
// the Experience, the broken block's six neighbors when a member broke, and nil otherwise.
func (h *Host) networkStarts(d *dispatcher, ev event, snap *snapshot) []cube.Pos {
	if !ev.call.anchored() || len(snap.cells) == 0 {
		return nil
	}
	if b := ev.call.Break; b != nil {
		broken, ok := h.reg.Lookup(b.Change.BeforeID)
		if !ok || broken.t.exp != d.id || !broken.t.network {
			return nil
		}
		starts := make([]cube.Pos, 0, len(cube.Faces()))
		for _, f := range cube.Faces() {
			starts = append(starts, ev.anchor.Side(f))
		}
		return starts
	}
	if anchor := snap.cells[0]; anchor.owned && anchor.block.t.network {
		return []cube.Pos{ev.anchor}
	}
	return nil
}
