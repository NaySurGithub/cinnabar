use crate::{MAX_TOUCH_CONTACTS, TouchContact, touch};

/// A control's activation rectangle in normalized viewport coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl TouchBounds {
    fn contains(self, position: [f32; 2]) -> bool {
        (0..2).all(|axis| self.min[axis] <= position[axis] && position[axis] <= self.max[axis])
    }

    fn valid(self) -> bool {
        (0..2).all(|axis| {
            self.min[axis].is_finite()
                && self.max[axis].is_finite()
                && self.min[axis] < self.max[axis]
        })
    }

    fn joystick_deflection(self, position: [f32; 2]) -> [f32; 2] {
        let mut delta = [0.0; 2];
        for axis in 0..2 {
            let radius = (self.max[axis] - self.min[axis]) * 0.5;
            let center = self.min[axis] + radius;
            delta[axis] = (position[axis] - center) / radius;
        }
        delta[1] = -delta[1];
        let magnitude = delta[0].hypot(delta[1]);
        if magnitude > 1.0 {
            delta = [delta[0] / magnitude, delta[1] / magnitude];
        }
        delta
    }
}

/// Supplied by the current UI layout; no viewport partition implies a control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchControlRegion {
    pub hit_id: u16,
    pub bounds: TouchBounds,
}

#[derive(Clone, Debug)]
struct CapturedContact {
    contact_id: u64,
    activity_sequence: u64,
    position: [f32; 2],
    look_delta: [f32; 2],
    region: Option<TouchControlRegion>,
}

/// Captures each finger at press time until release or cancellation.
#[derive(Clone, Debug, Default)]
pub struct TouchControlState {
    contacts: Vec<CapturedContact>,
}

impl TouchControlState {
    /// `None` excludes the contact for its entire lifetime, including UI-owned touches.
    pub fn begin(
        &mut self,
        contact_id: u64,
        position: [f32; 2],
        activity_sequence: u64,
        region: Option<TouchControlRegion>,
    ) -> bool {
        if self.contacts.len() >= MAX_TOUCH_CONTACTS
            || self
                .contacts
                .iter()
                .any(|held| held.contact_id == contact_id)
        {
            return false;
        }
        let region = region.filter(|region| {
            valid_position(position)
                && region.hit_id != 0
                && region.bounds.valid()
                && region.bounds.contains(position)
                && (!matches!(region.hit_id, touch::JOYSTICK | touch::LOOK_SURFACE)
                    || !self.contacts.iter().any(|held| {
                        held.region
                            .is_some_and(|previous| previous.hit_id == region.hit_id)
                    }))
        });
        self.contacts.push(CapturedContact {
            contact_id,
            activity_sequence,
            position,
            look_delta: [0.0; 2],
            region,
        });
        region.is_some()
    }

    /// Movement retains the initial control; an uncaptured move never acquires one.
    pub fn move_to(&mut self, contact_id: u64, position: [f32; 2], activity_sequence: u64) -> bool {
        if !valid_position(position) {
            self.release(contact_id);
            return false;
        }
        let Some(contact) = self
            .contacts
            .iter_mut()
            .find(|contact| contact.contact_id == contact_id)
        else {
            return false;
        };
        if position != contact.position {
            if contact
                .region
                .is_some_and(|region| region.hit_id == touch::LOOK_SURFACE)
            {
                for (axis, value) in position.into_iter().enumerate() {
                    contact.look_delta[axis] += value - contact.position[axis];
                }
            }
            contact.position = position;
            contact.activity_sequence = activity_sequence;
        }
        contact.region.is_some()
    }

    /// Handles either finger-up or finger-cancel without transferring ownership.
    pub fn release(&mut self, contact_id: u64) {
        self.contacts
            .retain(|contact| contact.contact_id != contact_id);
    }

    /// Discards all owners and pending look motion on focus or input-context loss.
    pub fn clear(&mut self) {
        self.contacts.clear();
    }

    /// Look motion is consumed once; joystick delta stays held with positive Y forward.
    pub fn sample(&mut self) -> Vec<TouchContact> {
        self.contacts
            .iter_mut()
            .filter_map(|contact| {
                let region = contact.region?;
                let delta = match region.hit_id {
                    touch::JOYSTICK => region.bounds.joystick_deflection(contact.position),
                    touch::LOOK_SURFACE => std::mem::take(&mut contact.look_delta),
                    _ => [0.0; 2],
                };
                Some(TouchContact {
                    contact_id: contact.contact_id,
                    activity_sequence: contact.activity_sequence,
                    position: contact.position,
                    delta,
                    hit_id: Some(region.hit_id),
                })
            })
            .collect()
    }
}

fn valid_position(position: [f32; 2]) -> bool {
    position
        .iter()
        .all(|axis| axis.is_finite() && (0.0..=1.0).contains(axis))
}

#[cfg(test)]
mod tests;
