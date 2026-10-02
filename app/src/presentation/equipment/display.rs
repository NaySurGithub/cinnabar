//! Native equipment placement uses the shared renderer owner.

pub(crate) use render::equipment_display::FirstPersonHand;
pub(super) use render::equipment_display::{
    FirstPersonShape, ItemDisplay, LAYER_BOOTS, LAYER_CHESTPLATE, LAYER_HELMET, LAYER_LEGGINGS,
    LAYER_MAIN_HAND, LAYER_OFF_HAND, attach_to_bone, first_person_display, head_block_display,
    held_block_display, held_sprite_display, is_hand_equipped, is_mirrored_art, view_bone,
};
