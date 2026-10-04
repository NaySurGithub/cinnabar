use json_ui::{DataSource, Scalar, ViewState, hit_test};
use semantic_input::{TouchBounds, TouchContact, TouchControlRegion, touch};
use ui::{UiNode, UiPoint};

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationError, UiPresentationRuntime,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};

const CONTROLS: &[(&str, u16)] = &[
    ("look", touch::LOOK_SURFACE),
    ("joystick", touch::JOYSTICK),
    ("jump", touch::JUMP),
    ("sneak", touch::SNEAK),
    ("sprint", touch::SPRINT),
    ("attack", touch::ATTACK),
    ("use", touch::USE),
    ("menu", touch::MENU),
    ("inventory", touch::INVENTORY),
    ("chat", touch::CHAT),
];

#[derive(Default)]
pub(super) struct TouchPresentation {
    pub(super) enabled: bool,
    screen: super::hud::CachedScreen,
    frame: Option<EngineFrame>,
    pub(super) hotbar: Option<EngineFrame>,
    contacts: Vec<TouchContact>,
}

impl UiPresentationRuntime {
    pub fn enable_touch_controls(&mut self, enabled: bool) {
        self.form_presentation.hud.touch.enabled = enabled;
    }

    pub fn touch_controls_enabled(&self) -> bool {
        self.form_presentation.hud.touch.enabled
    }

    pub fn set_touch_contacts(&mut self, contacts: Vec<TouchContact>) {
        self.form_presentation.hud.touch.contacts = contacts;
    }

    /// Regions use the same laid-out, clipped rectangles that were painted.
    pub fn gameplay_touch_region(
        &self,
        position: [f32; 2],
        window: [f32; 2],
    ) -> Option<TouchControlRegion> {
        let state = &self.form_presentation.hud.touch;
        if !state.enabled || window.iter().any(|size| !size.is_finite() || *size <= 0.0) {
            return None;
        }
        let point = UiPoint::new(position[0] * window[0], position[1] * window[1]).ok()?;
        if let Some(frame) = &state.hotbar {
            let region = hit_test(&frame.hits, frame.to_virtual(point));
            if let Some(region) =
                region.filter(|hit| hit.collection.as_deref() == Some("hotbar_items"))
            {
                let index = region.collection_index.filter(|index| *index < 9)?;
                return region_bounds(frame, region, window, touch::HOTBAR_1 + index as u16);
            }
        }
        let frame = state.frame.as_ref()?;
        let region = hit_test(&frame.hits, frame.to_virtual(point))?;
        let hit_id = control_id(region.pressed.as_deref())
            .or_else(|| (region.name == "knob").then_some(touch::JOYSTICK))?;
        let region = if hit_id == touch::JOYSTICK {
            frame
                .hits
                .iter()
                .find(|region| control_id(region.pressed.as_deref()) == Some(hit_id))?
        } else {
            region
        };
        region_bounds(frame, region, window, hit_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_touch_controls(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let state = &mut self.form_presentation.hud.touch;
        if !state.enabled {
            return Ok(());
        }
        let Some(renderer) = self.form_presentation.engine.as_ref() else {
            return Ok(());
        };
        let mut view = ViewState::default();
        if let Some(previous) = &state.frame
            && let Some(stick) = state
                .contacts
                .iter()
                .find(|contact| contact.hit_id == Some(touch::JOYSTICK))
            && let Some(knob) = previous.hits.iter().find(|hit| hit.name == "knob")
        {
            view.drags.insert(
                knob.key.clone(),
                [
                    f64::from(stick.delta[0]) * knob.rect.w,
                    -f64::from(stick.delta[1]) * knob.rect.h,
                ],
            );
        }
        let context = renderer.context().clone();
        let catalog = renderer.catalog().clone();
        let translate = |key: &str| runtime.translation(key);
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let mut data = DataSource::new();
        for &(name, id) in CONTROLS {
            let held = state
                .contacts
                .iter()
                .any(|contact| contact.hit_id == Some(id));
            data.set_global(format!("#touch_{name}_pressed"), Scalar::Bool(held));
            data.set_global(format!("#touch_{name}_normal"), Scalar::Bool(!held));
        }
        state.frame = renderer.draw(
            ScreenArt {
                now: now_millis as f64 / 1000.0,
                view: Some(&view),
                ..Default::default()
            },
            EngineInputs {
                layouts: &mut self.layouts,
                font: &self.font,
                metrics,
                solid_page: self.solid_texture_page,
                safe_area: self.safe_area,
                content,
                translate: &translate,
                language: runtime.text_generation(),
            },
            EngineOutput {
                nodes,
                next,
                overlay: &[],
            },
            |env, root| {
                state.screen.render_with(
                    "cinnabar_touch.screen",
                    &catalog,
                    &context,
                    data,
                    (root, px, runtime.text_generation()),
                    env,
                    &view,
                )
            },
        )?;
        Ok(())
    }
}

fn region_bounds(
    frame: &EngineFrame,
    region: &json_ui::HitRegion,
    window: [f32; 2],
    hit_id: u16,
) -> Option<TouchControlRegion> {
    let left = region.rect.x.max(region.clip.x);
    let top = region.rect.y.max(region.clip.y);
    let right = (region.rect.x + region.rect.w).min(region.clip.x + region.clip.w);
    let bottom = (region.rect.y + region.rect.h).min(region.clip.y + region.clip.h);
    if left >= right || top >= bottom {
        return None;
    }
    let normalized = |x: f64, y: f64| {
        [
            (frame.origin[0] + x as f32 * frame.scale) / window[0],
            (frame.origin[1] + y as f32 * frame.scale) / window[1],
        ]
    };
    Some(TouchControlRegion {
        hit_id,
        bounds: TouchBounds {
            min: normalized(left, top),
            max: normalized(right, bottom),
        },
    })
}

fn control_id(pressed: Option<&str>) -> Option<u16> {
    let name = pressed?.strip_prefix("cinnabar.touch.")?;
    CONTROLS
        .iter()
        .find_map(|&(control, id)| (control == name).then_some(id))
}
