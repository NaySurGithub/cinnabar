package showcase

import (
	"math"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl64"
)

// rayBox returns the distance along a unit dir at which a ray from origin enters box, within maxDist.
func rayBox(origin, dir mgl64.Vec3, box cube.BBox, maxDist float64) (float64, bool) {
	tMin, tMax := 0.0, maxDist
	lo, hi := box.Min(), box.Max()
	for axis := range 3 {
		if math.Abs(dir[axis]) < 1e-9 {
			if origin[axis] < lo[axis] || origin[axis] > hi[axis] {
				return 0, false
			}
			continue
		}
		t1, t2 := (lo[axis]-origin[axis])/dir[axis], (hi[axis]-origin[axis])/dir[axis]
		if t1 > t2 {
			t1, t2 = t2, t1
		}
		tMin, tMax = max(tMin, t1), min(tMax, t2)
		if tMin > tMax {
			return 0, false
		}
	}
	return tMin, true
}

// yawTowards is the Bedrock yaw, in degrees, of an entity at from facing to.
func yawTowards(from, to mgl64.Vec3) float64 {
	return mgl64.RadToDeg(math.Atan2(-(to[0] - from[0]), to[2]-from[2]))
}

// yawDir is the horizontal unit vector an entity with yaw faces.
func yawDir(yaw float64) mgl64.Vec3 {
	r := mgl64.DegToRad(yaw)
	return mgl64.Vec3{-math.Sin(r), 0, math.Cos(r)}
}

// flat drops the vertical component.
func flat(v mgl64.Vec3) mgl64.Vec3 { return mgl64.Vec3{v[0], 0, v[2]} }

// flatDist is the horizontal distance between two points.
func flatDist(a, b mgl64.Vec3) float64 { return flat(a.Sub(b)).Len() }

// inArc reports whether target lies within radius of origin and within halfAngle degrees of yaw.
func inArc(origin mgl64.Vec3, yaw float64, target mgl64.Vec3, radius, halfAngle float64) bool {
	d := flat(target.Sub(origin))
	if d.Len() > radius {
		return false
	}
	if d.Len() < 1e-6 {
		return true
	}
	return d.Normalize().Dot(yawDir(yaw)) >= math.Cos(mgl64.DegToRad(halfAngle))
}
