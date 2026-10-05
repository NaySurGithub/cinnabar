package showcase

import (
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	// BossIdentifier is the entity type the pack's client entity defines.
	BossIdentifier = "cinnabar:hollow_warden"
	// BossName is shown on the boss bar.
	BossName  = "Varr, the Hollow Warden"
	bossScale = 1.6
	// Vanilla warden collision box, scaled.
	bossHalfWidth = 0.45 * bossScale
	bossHeight    = 2.9 * bossScale
)

// BossType is the world.EntityType of the hollow warden. The controller drives it, so it never ticks
// itself and is never restored from a save.
var BossType bossType

type bossType struct{}

func (bossType) Open(tx *world.Tx, handle *world.EntityHandle, data *world.EntityData) world.Entity {
	return &bossEntity{Ent: entity.Open(tx, handle, data), data: data}
}
func (bossType) EncodeEntity() string { return BossIdentifier }
func (bossType) BBox(world.Entity) cube.BBox {
	return cube.Box(-bossHalfWidth, 0, -bossHalfWidth, bossHalfWidth, bossHeight, bossHalfWidth)
}
func (bossType) DecodeNBT(_ map[string]any, data *world.EntityData) { data.Data = idleBehaviour{} }
func (bossType) EncodeNBT(*world.EntityData) map[string]any         { return nil }

// idleBehaviour leaves movement to the controller.
type idleBehaviour struct{}

func (idleBehaviour) Tick(*entity.Ent, *world.Tx) *entity.Movement { return nil }

type bossConfig struct{}

func (bossConfig) Apply(data *world.EntityData) {
	data.Data = idleBehaviour{}
	data.AlwaysShowNameTag = false
}

// newBossHandle returns a boss handle to add at pos.
func newBossHandle(pos mgl64.Vec3, yaw float64) *world.EntityHandle {
	return world.EntitySpawnOpts{Position: pos, Rotation: cube.Rotation{yaw, 0}}.New(BossType, bossConfig{})
}

// bossEntity exposes the metadata scale and lets the controller move the boss smoothly.
type bossEntity struct {
	*entity.Ent
	data *world.EntityData
}

// Scale is read by the session's entity metadata.
func (b *bossEntity) Scale() float64 { return bossScale }

// move applies one tick of velocity with gravity and collisions, facing yaw, and shows it to viewers.
func (b *bossEntity) move(tx *world.Tx, mc *entity.MovementComputer, yaw float64) {
	oldPos, oldRot := b.data.Pos, b.data.Rot
	m := mc.TickMovement(b, oldPos, b.data.Vel, cube.Rotation{yaw, 0}, tx)
	b.data.Pos, b.data.Vel, b.data.Rot = m.Position(), m.Velocity(), m.Rotation()
	m.Send()
	// Send only reports position and velocity changes; a turn on the spot needs its own update.
	if m.Position() == oldPos && m.Rotation() != oldRot {
		for _, v := range tx.Viewers(oldPos) {
			v.ViewEntityMovement(b, oldPos, m.Rotation(), mc.OnGround())
		}
	}
}

// bbox is the boss's collision box in world space.
func (b *bossEntity) bbox() cube.BBox { return BossType.BBox(b).Translate(b.data.Pos) }
