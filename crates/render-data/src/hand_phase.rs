/// The arm's swing and equip progress over one tick, as the first-person item reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPhase {
    /// 0..1 swing progress; 0 at rest.
    pub attack_time: f32,
    /// 0..1 equip progress; 1 once the held item has settled.
    pub arm_height: f32,
    /// Consecutive ticks the using-item flag has been set.
    pub use_ticks: u32,
}

impl Default for HandPhase {
    fn default() -> Self {
        Self {
            attack_time: 0.0,
            arm_height: 1.0,
            use_ticks: 0,
        }
    }
}
