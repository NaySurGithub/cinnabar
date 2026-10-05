package showcase

import (
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

func blockAt(t *testing.T, a Arena, pos cube.Pos) world.Block {
	t.Helper()
	b, ok := a.Block(pos)
	if !ok {
		t.Fatalf("%v is outside the arena volume", pos)
	}
	return b
}

func isAir(b world.Block) bool { _, ok := b.(block.Air); return ok }

func TestArenaWallsEncloseTheFloorExceptTheEntrance(t *testing.T) {
	a := Arena{Origin: cube.Pos{100, 64, -30}}
	y := a.floorY() + 1
	gaps := 0
	for i := -arenaHalf; i <= arenaHalf; i++ {
		for _, pos := range []cube.Pos{
			{100 + i, y, -30 - arenaHalf}, {100 + i, y, -30 + arenaHalf},
			{100 - arenaHalf, y, -30 + i}, {100 + arenaHalf, y, -30 + i},
		} {
			if isAir(blockAt(t, a, pos)) {
				gaps++
				if pos.Z() != -30+arenaHalf || abs(pos.X()-100) > entranceHalf {
					t.Errorf("gap in the wall at %v", pos)
				}
			}
		}
	}
	if gaps != 2*entranceHalf+1 {
		t.Fatalf("entrance is %d blocks wide, want %d", gaps, 2*entranceHalf+1)
	}
	if isAir(blockAt(t, a, cube.Pos{100, y + entranceHeight, -30 + arenaHalf})) {
		t.Fatal("entrance has no lintel")
	}
}

func TestArenaFloorIsSolidAndTheInteriorClear(t *testing.T) {
	a := Arena{Origin: cube.Pos{0, 10, 0}}
	for x := -arenaHalf + 1; x < arenaHalf; x++ {
		for z := -arenaHalf + 1; z < arenaHalf; z++ {
			if isAir(blockAt(t, a, cube.Pos{x, 9, z})) {
				t.Fatalf("hole in the floor at %d,%d", x, z)
			}
			if b := blockAt(t, a, cube.Pos{x, 12, z}); !isAir(b) {
				t.Fatalf("obstruction %T inside the arena at %d,%d", b, x, z)
			}
		}
	}
}

func TestGraceAndSpawnsSitInsideTheArena(t *testing.T) {
	a := Arena{Origin: cube.Pos{5, 70, 5}}
	g := a.Grace()
	if _, ok := blockAt(t, a, g).(block.Campfire); !ok {
		t.Fatalf("grace block is %T, want a campfire", blockAt(t, a, g))
	}
	if g.Z()-a.Origin.Z() < arenaHalf-4 {
		t.Fatal("grace is not by the entrance")
	}
	for name, pos := range map[string]cube.Pos{"player": cube.PosFromVec3(a.PlayerSpawn()), "boss": cube.PosFromVec3(a.BossSpawn())} {
		if !a.Contains(pos.Vec3Middle()) || !isAir(blockAt(t, a, pos)) || isAir(blockAt(t, a, pos.Side(cube.FaceDown))) {
			t.Errorf("%s spawn %v is not standing room inside the arena", name, pos)
		}
	}
	if _, ok := a.Block(cube.Pos{5 + arenaHalf + 1, 70, 5}); ok {
		t.Fatal("arena claims a block outside its walls")
	}
}
