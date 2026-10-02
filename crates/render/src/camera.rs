use bevy::math::{EulerRot, Quat};
use bevy::prelude::{Mat4, Resource};

pub const DEFAULT_HORIZONTAL_FOV_RADIANS: f32 = ui::DEFAULT_HORIZONTAL_FOV_DEGREES.to_radians();
const MIN_FOV_RADIANS: f32 = std::f32::consts::PI / 180.0;
const MAX_FOV_RADIANS: f32 = std::f32::consts::PI - MIN_FOV_RADIANS;
const DEFAULT_ASPECT_RATIO: f32 = 16.0 / 9.0;

/// Converts the user-facing horizontal FOV to Bevy's aspect-correct vertical
/// FOV while keeping malformed or zero-size window input finite and valid.
#[must_use]
pub fn horizontal_fov_to_vertical(horizontal: f32, aspect: f32) -> f32 {
    let horizontal = if horizontal.is_finite() {
        horizontal.clamp(MIN_FOV_RADIANS, MAX_FOV_RADIANS)
    } else {
        DEFAULT_HORIZONTAL_FOV_RADIANS
    };
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        DEFAULT_ASPECT_RATIO
    };
    (2.0 * ((horizontal * 0.5).tan() / aspect).atan()).clamp(MIN_FOV_RADIANS, MAX_FOV_RADIANS)
}

mod bob;
mod hurt;
pub use bob::{HandSwayState, ViewEffect, WalkBobState, walk_bob_effect};
pub use hurt::{CameraHurtState, LocalHurtEvent};

/// First-person hand motion for the equipment lane, all in view space.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FirstPersonHandMotion {
    pub bob: ViewEffect,
    pub hurt: Mat4,
    pub sway_pitch_radians: f32,
    pub sway_yaw_radians: f32,
}

impl Default for FirstPersonHandMotion {
    fn default() -> Self {
        Self {
            bob: ViewEffect::NONE,
            hurt: Mat4::IDENTITY,
            sway_pitch_radians: 0.0,
            sway_yaw_radians: 0.0,
        }
    }
}

impl FirstPersonHandMotion {
    /// Native world view effects exclude the hand-only turn spring.
    #[must_use]
    pub fn view_matrix(&self) -> Mat4 {
        self.hurt * self.bob.matrix()
    }

    /// Native hand stack order: hurt tilt, walk bob, then sway about X and Y.
    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        self.view_matrix()
            * Mat4::from_rotation_x(self.sway_pitch_radians)
            * Mat4::from_rotation_y(self.sway_yaw_radians)
    }
}

/// Converts the Bedrock degree convention into the renderer's right-handed view rotation.
#[must_use]
pub fn bedrock_camera_rotation(yaw_degrees: f32, pitch_degrees: f32) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        (180.0 - yaw_degrees).to_radians(),
        -pitch_degrees.to_radians(),
        0.0,
    )
}
