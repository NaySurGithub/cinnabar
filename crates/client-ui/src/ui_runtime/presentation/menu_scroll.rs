//! Menu scroll views: offsets kept across frames, the areas the last frame
//! drew (window-logical), and wheel, scrollbar-drag and track-press input.

use std::{collections::HashMap, time::Instant};

use ui::{UiPoint, UiRect};

/// One scroll view as drawn; offsets are in the drawing system's own units.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollArea {
    pub key: String,
    pub viewport: UiRect,
    /// Window-logical pixels per offset unit.
    pub scale: f32,
    pub offset: f32,
    pub max: f32,
    /// Offset units per wheel notch.
    pub speed: f32,
    pub track: Option<UiRect>,
    pub thumb: Option<UiRect>,
    /// A JSON-UI view's metrics and the window point of its virtual origin: its
    /// input follows the client's scroll rules.
    pub engine: Option<(json_ui::ScrollMetrics, [f32; 2])>,
    /// Whether the box can be grabbed (its `draggable` is not `not_draggable`).
    pub draggable: bool,
}

impl ScrollArea {
    /// The engine metrics at the current offset, and `point` in virtual pixels.
    fn engine_at(&self, point: UiPoint) -> Option<(json_ui::ScrollMetrics, [f64; 2])> {
        let (metrics, origin) = self.engine.as_ref()?;
        let metrics = json_ui::ScrollMetrics {
            offset: f64::from(self.offset),
            ..metrics.clone()
        };
        let virtual_at = |value: f32, axis: usize| f64::from((value - origin[axis]) / self.scale);
        Some((
            metrics,
            [virtual_at(point.x(), 0), virtual_at(point.y(), 1)],
        ))
    }

    /// The offset that puts the thumb's top at window `y`.
    fn offset_for_thumb(&self, y: f32) -> f32 {
        let (Some(track), Some(thumb)) = (self.track, self.thumb) else {
            return self.offset;
        };
        let travel = track.height() - thumb.height();
        if travel <= 0.0 {
            return self.offset;
        }
        ((y - track.min().y()) / travel * self.max).clamp(0.0, self.max)
    }
}

#[derive(Default)]
pub struct MenuScrolls {
    offsets: HashMap<String, f32>,
    areas: Vec<ScrollArea>,
    /// The dragged view and the grab point's distance below its thumb's top
    /// (an engine view: the pointer's last virtual position along its axis).
    drag: Option<(String, f32)>,
    screen: Option<String>,
    focused: Option<crate::menu::MenuAction>,
    touch: Option<(String, UiPoint)>,
    motion: json_ui::ViewState,
    clock: Option<Instant>,
}

impl MenuScrolls {
    /// Forget every offset when the menu shows another screen.
    pub fn begin_frame(&mut self, screen: String) {
        if self.screen.as_ref() != Some(&screen) {
            self.offsets.clear();
            self.drag = None;
            self.screen = Some(screen);
            self.focused = None;
            self.touch = None;
            self.motion = json_ui::ViewState::default();
            self.clock = None;
        }
        let now = Instant::now();
        let dt = self
            .clock
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.clock = Some(now);
        self.step_touch(dt);
    }

    /// Reveals a newly focused fallback control without overriding later wheel movement.
    pub fn reveal_focus(
        &mut self,
        key: &str,
        action: Option<crate::menu::MenuAction>,
        bounds: Option<UiRect>,
        viewport: UiRect,
        max: f32,
    ) -> f32 {
        let mut offset = self
            .offsets
            .get(key)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, max);
        if self.focused != action {
            self.focused = action;
            if let Some(bounds) = bounds {
                let top = bounds.min().y() - offset;
                let bottom = bounds.max().y() - offset;
                if top < viewport.min().y() {
                    offset -= viewport.min().y() - top;
                } else if bottom > viewport.max().y() {
                    offset += bottom - viewport.max().y();
                }
                offset = offset.clamp(0.0, max);
                self.offsets.insert(key.to_owned(), offset);
            }
        }
        offset
    }

    /// A touch owns its initial view until release, including outside its viewport.
    pub fn begin_touch(&mut self, point: UiPoint) -> bool {
        let Some(area) = self.at(point).cloned() else {
            return false;
        };
        let metrics = Self::touch_metrics(&area);
        if !metrics.gesture {
            return false;
        }
        self.motion
            .scroll
            .insert(area.key.clone(), f64::from(area.offset));
        self.motion.begin_scroll_touch(&area.key, &metrics);
        self.touch = Some((area.key, point));
        true
    }

    pub fn move_touch(&mut self, point: UiPoint) {
        let Some((key, previous)) = self.touch.clone() else {
            return;
        };
        let Some(area) = self.areas.iter().find(|area| area.key == key) else {
            return;
        };
        let delta = [
            f64::from((point.x() - previous.x()) / area.scale),
            f64::from((point.y() - previous.y()) / area.scale),
        ];
        self.motion
            .scroll_touch_moved(&key, &Self::touch_metrics(area), delta);
        self.touch = Some((key, point));
    }

    /// Returns whether the gesture still counts as a tap.
    pub fn end_touch(&mut self) -> bool {
        self.touch
            .take()
            .is_none_or(|(key, _)| self.motion.end_scroll_touch(&key))
    }

    pub fn cancel_touch(&mut self) {
        if let Some((key, _)) = self.touch.take() {
            self.motion.scroll_state.remove(&key);
        }
    }

    fn touch_metrics(area: &ScrollArea) -> json_ui::ScrollMetrics {
        area.engine.as_ref().map_or_else(
            || json_ui::ScrollMetrics {
                offset: f64::from(area.offset),
                content: f64::from(area.viewport.height() / area.scale + area.max),
                viewport: f64::from(area.viewport.height() / area.scale),
                gesture: true,
                touch_mode: true,
                ..Default::default()
            },
            |(metrics, _)| metrics.clone(),
        )
    }

    fn step_touch(&mut self, dt: f64) {
        if !self
            .motion
            .scroll_state
            .values()
            .any(|state| state.motion.is_some() || state.bar_fade.is_some_and(|alpha| alpha > 0.0))
        {
            return;
        }
        let report = json_ui::LayoutReport {
            scrolls: self
                .areas
                .iter()
                .map(|area| (area.key.clone(), Self::touch_metrics(area)))
                .collect(),
            ..Default::default()
        };
        self.motion.step_scrolls(&report, dt);
        for key in self.motion.scroll_state.keys() {
            let offset = self.motion.scroll_offset(key);
            if let Some(area) = self.areas.iter_mut().find(|area| area.key == *key) {
                area.offset = offset as f32;
                self.offsets.insert(key.clone(), area.offset);
            }
        }
    }

    pub(super) fn touch_view(&self) -> &json_ui::ViewState {
        &self.motion
    }

    pub fn offsets(&self) -> &HashMap<String, f32> {
        &self.offsets
    }

    pub fn set_areas(&mut self, areas: Vec<ScrollArea>) {
        self.areas = areas;
    }

    fn at(&self, point: UiPoint) -> Option<&ScrollArea> {
        self.areas
            .iter()
            .rev()
            .find(|area| area.viewport.contains(point))
    }

    fn set(&mut self, key: &str, offset: f32) {
        self.motion.scroll_state.remove(key);
        if let Some(area) = self.areas.iter_mut().find(|area| area.key == key) {
            area.offset = offset.clamp(0.0, area.max);
            self.offsets.insert(key.to_owned(), area.offset);
        }
    }

    /// Scrolls the view under `point` by `notches` (lines) or window pixels.
    pub fn wheel(&mut self, point: UiPoint, notches: f32, pixels: bool) -> bool {
        let Some(area) = self.at(point) else {
            return false;
        };
        let offset = match area.engine_at(point) {
            Some((metrics, _)) if !pixels => metrics.offset_for_wheel(f64::from(notches)) as f32,
            _ if pixels => area.offset - notches / area.scale,
            _ => area.offset - notches * area.speed,
        };
        let key = area.key.clone();
        self.set(&key, offset);
        true
    }

    /// A press on a scrollbar: grabs a draggable thumb, or centres the view on
    /// the track fraction pressed. `true` when the press belonged to a scrollbar.
    pub fn press(&mut self, point: UiPoint) -> bool {
        let Some(area) = self
            .areas
            .iter()
            .rev()
            .find(|area| area.track.is_some_and(|track| track.contains(point)))
        else {
            return false;
        };
        let key = area.key.clone();
        if let Some((metrics, at)) = area.engine_at(point) {
            let along = at[usize::from(!metrics.horizontal)] as f32;
            match area.thumb {
                Some(thumb) if thumb.contains(point) => {
                    if area.draggable {
                        self.drag = Some((key, along));
                    }
                }
                // A track press jumps only when the view names its track button.
                _ if metrics.track_button.is_some() => {
                    self.set(&key, metrics.offset_for_track(at) as f32);
                }
                _ => {}
            }
            return true;
        }
        match (area.thumb, area.track) {
            (Some(thumb), _) if thumb.contains(point) => {
                if area.draggable {
                    self.drag = Some((key, point.y() - thumb.min().y()));
                }
            }
            (_, Some(track)) => {
                let view = area.viewport.height() / area.scale;
                let fraction = if track.height() > 0.5 * area.scale {
                    (point.y() - track.min().y()) / track.height()
                } else {
                    1.0
                };
                self.set(&key, view * -0.5 + fraction * (area.max + view));
            }
            _ => {}
        }
        true
    }

    /// Follows a held thumb drag; a release ends it.
    pub fn drag(&mut self, point: Option<UiPoint>, held: bool) {
        if !held {
            self.drag = None;
            return;
        }
        let (Some((key, grab)), Some(point)) = (self.drag.clone(), point) else {
            return;
        };
        let Some(area) = self.areas.iter().find(|area| area.key == key) else {
            return;
        };
        if let Some((metrics, at)) = area.engine_at(point) {
            let along = at[usize::from(!metrics.horizontal)];
            let offset = metrics.thumb_drag_target(along - f64::from(grab)) as f32;
            self.set(&key, offset);
            self.drag = Some((key, along as f32));
            return;
        }
        let offset = area.offset_for_thumb(point.y() - grab);
        self.set(&key, offset);
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

impl super::UiPresentationRuntime {
    pub fn begin_menu_touch(&mut self, point: UiPoint) -> bool {
        self.menu_scrolls.begin_touch(point)
    }

    pub fn move_menu_touch(&mut self, point: UiPoint) {
        self.menu_scrolls.move_touch(point);
    }

    pub fn end_menu_touch(&mut self) -> bool {
        self.menu_scrolls.end_touch()
    }

    pub fn cancel_menu_touch(&mut self) {
        self.menu_scrolls.cancel_touch();
    }

    /// Scrolls the menu view under `point`; `true` when one took the wheel.
    pub fn scroll_menu(&mut self, point: UiPoint, notches: f32, pixels: bool) -> bool {
        self.menu_scrolls.wheel(point, notches, pixels)
    }

    /// A menu press on a scrollbar, which then takes no button action.
    pub fn press_menu_scrollbar(&mut self, point: UiPoint) -> bool {
        self.menu_scrolls.press(point)
    }

    /// Follows a held scrollbar drag; `true` while one is live.
    pub fn drag_menu_scroll(&mut self, point: Option<UiPoint>, held: bool) -> bool {
        self.menu_scrolls.drag(point, held);
        self.menu_scrolls.dragging()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> ScrollArea {
        let rect = |x0, y0, x1, y1| UiRect::new(point(x0, y0), point(x1, y1)).unwrap();
        ScrollArea {
            key: "list".to_owned(),
            viewport: rect(0.0, 0.0, 100.0, 100.0),
            scale: 2.0,
            offset: 0.0,
            max: 150.0,
            speed: 10.0,
            track: Some(rect(95.0, 0.0, 100.0, 100.0)),
            thumb: Some(rect(95.0, 0.0, 100.0, 25.0)),
            engine: None,
            draggable: true,
        }
    }

    fn point(x: f32, y: f32) -> UiPoint {
        UiPoint::new(x, y).unwrap()
    }

    #[test]
    fn touch_pan_uses_virtual_pixels_flings_and_never_counts_as_a_tap() {
        let mut scrolls = MenuScrolls::default();
        let mut view = area();
        view.engine = Some((
            json_ui::ScrollMetrics {
                content: 200.0,
                viewport: 50.0,
                gesture: true,
                touch_mode: true,
                ..Default::default()
            },
            [0.0, 0.0],
        ));
        scrolls.set_areas(vec![view]);
        assert!(!scrolls.begin_touch(point(150.0, 50.0)));
        assert!(scrolls.begin_touch(point(50.0, 80.0)));
        for index in 1..=20 {
            scrolls.move_touch(point(50.0, 80.0 - index as f32 * 4.0));
            scrolls.step_touch(1.0 / 60.0);
        }
        let held = scrolls.offsets()["list"];
        assert!(held > 20.0 && held < 50.0, "scaled pan: {held}");
        assert!(!scrolls.end_touch());
        for _ in 0..600 {
            scrolls.step_touch(1.0 / 60.0);
        }
        assert!(
            scrolls.offsets()["list"] > held,
            "release flings the content"
        );
        assert!(scrolls.offsets()["list"] <= 150.0);
        assert!(scrolls.motion.scroll_state["list"].motion.is_none());
        assert!(scrolls.begin_touch(point(50.0, 50.0)));
        assert!(scrolls.end_touch(), "stationary finger remains a tap");
    }

    #[test]
    fn touch_capture_survives_leaving_viewport_and_cancels_on_navigation() {
        let mut scrolls = MenuScrolls::default();
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.begin_touch(point(50.0, 80.0)));
        scrolls.move_touch(point(50.0, -40.0));
        scrolls.step_touch(0.1);
        assert!(scrolls.offsets()["list"] > 0.0);
        assert!(!scrolls.end_touch());
        scrolls.begin_frame("new screen".into());
        assert!(scrolls.offsets().is_empty());
        assert!(scrolls.motion.scroll_state.is_empty());
        assert!(scrolls.begin_touch(point(50.0, 50.0)));
        scrolls.cancel_touch();
        assert!(scrolls.motion.scroll_state.is_empty());
        assert!(scrolls.end_touch());
    }

    // Wheel, track press and thumb drag each move the view under the pointer,
    // clamped to its content; a track press centres on its fraction.
    #[test]
    fn wheel_track_and_thumb_scroll_the_view() {
        let mut scrolls = MenuScrolls::default();
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.wheel(point(50.0, 50.0), -2.0, false));
        assert_eq!(scrolls.offsets()["list"], 20.0);
        assert!(!scrolls.wheel(point(150.0, 50.0), -2.0, false));
        assert!(scrolls.press(point(97.0, 80.0)));
        assert_eq!(scrolls.offsets()["list"], 135.0);
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.press(point(97.0, 10.0)));
        scrolls.drag(Some(point(97.0, 85.0)), true);
        assert_eq!(scrolls.offsets()["list"], 150.0);
        scrolls.drag(None, false);
        assert!(!scrolls.dragging());
        scrolls.begin_frame("other".to_owned());
        assert!(scrolls.offsets().is_empty());
    }
}
