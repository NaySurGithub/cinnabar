package showcase

import (
	"math"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl64"
)

func TestRayBoxHitsWithinRangeOnly(t *testing.T) {
	box := cube.Box(4, 0, -1, 6, 2, 1)
	if d, ok := rayBox(mgl64.Vec3{0, 1, 0}, mgl64.Vec3{1, 0, 0}, box, 24); !ok || math.Abs(d-4) > 1e-9 {
		t.Fatalf("straight ray: %v %v", d, ok)
	}
	if _, ok := rayBox(mgl64.Vec3{0, 1, 0}, mgl64.Vec3{1, 0, 0}, box, 3); ok {
		t.Fatal("hit beyond range")
	}
	if _, ok := rayBox(mgl64.Vec3{0, 1, 0}, mgl64.Vec3{-1, 0, 0}, box, 24); ok {
		t.Fatal("hit behind the origin")
	}
	if _, ok := rayBox(mgl64.Vec3{0, 5, 0}, mgl64.Vec3{1, 0, 0}, box, 24); ok {
		t.Fatal("hit above the box")
	}
}

func TestYawConventionMatchesBedrock(t *testing.T) {
	for _, c := range []struct {
		to  mgl64.Vec3
		yaw float64
	}{{mgl64.Vec3{0, 0, 1}, 0}, {mgl64.Vec3{-1, 0, 0}, 90}, {mgl64.Vec3{1, 0, 0}, -90}} {
		if got := yawTowards(mgl64.Vec3{}, c.to); math.Abs(got-c.yaw) > 1e-9 {
			t.Errorf("yawTowards(%v) = %v, want %v", c.to, got, c.yaw)
		}
		if d := yawDir(c.yaw).Sub(c.to).Len(); d > 1e-9 {
			t.Errorf("yawDir(%v) off by %v", c.yaw, d)
		}
	}
}

func TestSwipeArcCoversOnlyTheFront(t *testing.T) {
	origin := mgl64.Vec3{}
	if !inArc(origin, 0, mgl64.Vec3{1, 0, 3}, swipeRadius, swipeHalfAngle) {
		t.Fatal("target in front missed")
	}
	if inArc(origin, 0, mgl64.Vec3{0, 0, -3}, swipeRadius, swipeHalfAngle) {
		t.Fatal("target behind hit")
	}
	if inArc(origin, 0, mgl64.Vec3{0, 0, swipeRadius + 1}, swipeRadius, swipeHalfAngle) {
		t.Fatal("target out of reach hit")
	}
}
