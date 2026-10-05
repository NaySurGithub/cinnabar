package showcase

import (
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	arenaHalf      = 20 // walls stand at ±arenaHalf around the origin
	wallHeight     = 7
	pillarHeight   = 9
	pillarSpacing  = 8
	clearHeight    = 16 // air above the floor inside the build volume
	entranceHalf   = 1
	entranceHeight = 4
)

// Arena is the walled stone ring generated around the player that ran /showcase souls.
type Arena struct {
	Origin cube.Pos // the block the player stood in; the floor is the layer below
}

func (a Arena) floorY() int { return a.Origin.Y() - 1 }

// Grace is the site of grace, just inside the south entrance.
func (a Arena) Grace() cube.Pos {
	return cube.Pos{a.Origin.X(), a.floorY() + 1, a.Origin.Z() + arenaHalf - 3}
}

// PlayerSpawn is where players start and respawn, facing the boss.
func (a Arena) PlayerSpawn() mgl64.Vec3 {
	return cube.Pos{a.Origin.X(), a.floorY() + 1, a.Origin.Z() + arenaHalf - 5}.Vec3Middle()
}

// BossSpawn is the boss's starting point in the north half.
func (a Arena) BossSpawn() mgl64.Vec3 {
	return cube.Pos{a.Origin.X(), a.floorY() + 1, a.Origin.Z() - 8}.Vec3Middle()
}

// Contains reports whether pos is inside the walls, up to the cleared height.
func (a Arena) Contains(pos mgl64.Vec3) bool {
	dx, dz := pos[0]-float64(a.Origin.X())-0.5, pos[2]-float64(a.Origin.Z())-0.5
	dy := pos[1] - float64(a.floorY())
	return math.Abs(dx) < arenaHalf && math.Abs(dz) < arenaHalf && dy >= 0 && dy <= clearHeight+8
}

// bounds returns the corners of the build volume.
func (a Arena) bounds() (lo, hi cube.Pos) {
	o, y := a.Origin, a.floorY()
	return cube.Pos{o.X() - arenaHalf, y - 1, o.Z() - arenaHalf}, cube.Pos{o.X() + arenaHalf, y + clearHeight, o.Z() + arenaHalf}
}

// Block returns what the arena places at pos; positions outside the build volume report false.
func (a Arena) Block(pos cube.Pos) (world.Block, bool) {
	lo, hi := a.bounds()
	if pos.X() < lo.X() || pos.X() > hi.X() || pos.Y() < lo.Y() || pos.Y() > hi.Y() || pos.Z() < lo.Z() || pos.Z() > hi.Z() {
		return nil, false
	}
	x, z := pos.X()-a.Origin.X(), pos.Z()-a.Origin.Z()
	h := pos.Y() - a.floorY()
	edge := abs(x) == arenaHalf || abs(z) == arenaHalf
	entrance := z == arenaHalf && abs(x) <= entranceHalf
	switch {
	case h < 0:
		return block.Deepslate{}, true
	case h == 0:
		return a.floorBlock(x, z), true
	case edge && pillar(x, z) && !entrance:
		switch {
		case h <= pillarHeight:
			return block.Blackstone{Type: block.ChiseledPolishedBlackstone()}, true
		case h == pillarHeight+1:
			return block.Lantern{Type: block.SoulFire()}, true
		}
	case edge:
		if entrance && h <= entranceHeight {
			return block.Air{}, true
		}
		if h <= wallHeight {
			return block.DeepslateBricks{Cracked: hash(x, z, h)%7 == 0}, true
		}
	case pos == a.Grace():
		return block.Campfire{Type: block.SoulFire(), Facing: cube.North}, true
	}
	return block.Air{}, true
}

// floorBlock lays stone bricks with blackstone rings and a gilded centre sigil.
func (a Arena) floorBlock(x, z int) world.Block {
	if g := a.Grace(); x == g.X()-a.Origin.X() && z == g.Z()-a.Origin.Z() {
		return block.Blackstone{Type: block.GildedBlackstone()}
	}
	if abs(x) == arenaHalf || abs(z) == arenaHalf {
		return block.PolishedBlackstoneBrick{}
	}
	r := math.Hypot(float64(x), float64(z))
	switch {
	case r <= 1.5:
		return block.Blackstone{Type: block.GildedBlackstone()}
	case r <= 2.5:
		return block.Blackstone{Type: block.ChiseledPolishedBlackstone()}
	case math.Abs(r-7) < 0.5, math.Abs(r-14) < 0.5:
		return block.PolishedBlackstoneBrick{Cracked: hash(x, z, 0)%5 == 0}
	}
	switch n := hash(x, z, 0) % 20; {
	case n < 3:
		return block.StoneBricks{Type: block.CrackedStoneBricks()}
	case n < 5:
		return block.StoneBricks{Type: block.MossyStoneBricks()}
	}
	return block.StoneBricks{Type: block.NormalStoneBricks()}
}

// Build places the whole arena; existing blocks in the volume are replaced.
func (a Arena) Build(tx *world.Tx) {
	lo, hi := a.bounds()
	opts := &world.SetOpts{DisableBlockUpdates: true, DisableLiquidDisplacement: true}
	for pos := range cube.Range3D(lo, hi) {
		if b, ok := a.Block(pos); ok {
			tx.SetBlock(pos, b, opts)
		}
	}
}

// pillar reports whether a wall column carries a pillar: the corners and every pillarSpacing blocks.
func pillar(x, z int) bool {
	return (abs(x) == arenaHalf || x%pillarSpacing == 0) && (abs(z) == arenaHalf || z%pillarSpacing == 0)
}

// hash is a small deterministic mix so the floor pattern is the same every build.
func hash(x, z, y int) uint32 {
	h := uint32(x)*73856093 ^ uint32(z)*19349663 ^ uint32(y)*83492791
	h ^= h >> 13
	h *= 0x5bd1e995
	return h ^ h>>15
}

func abs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
