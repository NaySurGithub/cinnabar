//! Touch input uses the shipped overlay's painted, clipped control geometry.
use json_ui::{
    Catalog, Context, DataSource, Draw, EmptyLibrary, FormRender, HitRegion, LayoutEnv, RectOut,
    Scalar, TextMeasure, TextureMeta, TextureSource, ViewState, bind, hit_test, render_bound,
    resolve,
};

struct NoText;
impl TextMeasure for NoText {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}

struct Icons;
impl TextureSource for Icons {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        path.starts_with("textures/ui/")
            .then(|| TextureMeta::plain([16.0; 2]))
    }
}

fn render(size: [f64; 2], view: &ViewState) -> FormRender {
    render_held(size, view, &[])
}

fn render_held(size: [f64; 2], view: &ViewState, pressed: &[&str]) -> FormRender {
    let mut catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":[]}"#.as_slice()),
    ])
    .unwrap();
    let index = include_bytes!("../../../assets/touch-ui/ui/_ui_defs.json").as_slice();
    let paths = Catalog::declared_paths(index).unwrap();
    catalog.apply_pack([
        ("ui/_ui_defs.json", index),
        (
            paths[0].as_str(),
            include_bytes!("../../../assets/touch-ui/ui/touch_controls.json").as_slice(),
        ),
    ]);
    let result = resolve(&catalog, "cinnabar_touch.screen", &Context::android());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let root = result.control.expect("Android touch overlay");
    let mut data = DataSource::new();
    for control in &root.children {
        let held = pressed.contains(&control.name.as_str());
        data.set_global(
            format!("#touch_{}_pressed", control.name),
            Scalar::Bool(held),
        );
        data.set_global(
            format!("#touch_{}_normal", control.name),
            Scalar::Bool(!held),
        );
    }
    render_bound(
        bind(&root, &data, &EmptyLibrary),
        size,
        &LayoutEnv {
            text: &NoText,
            textures: &Icons,
        },
        view,
    )
}

fn visible(rect: RectOut, clip: RectOut) -> Option<RectOut> {
    let x = rect.x.max(clip.x);
    let y = rect.y.max(clip.y);
    let right = (rect.x + rect.w).min(clip.x + clip.w);
    let bottom = (rect.y + rect.h).min(clip.y + clip.h);
    (x < right && y < bottom).then_some(RectOut {
        x,
        y,
        w: right - x,
        h: bottom - y,
    })
}

fn control<'a>(render: &'a FormRender, action: &str) -> &'a HitRegion {
    render
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some(action))
        .unwrap()
}

#[test]
fn android_buttons_hit_their_painted_rectangles_and_leave_the_rest_for_look() {
    for size in [[320.0, 180.0], [240.0, 320.0]] {
        let view = ViewState::default();
        let frame = render(size, &view);
        for hit in frame.hits.iter().filter(|hit| {
            hit.pressed.is_some() && hit.pressed.as_deref() != Some("cinnabar.touch.look")
        }) {
            let sprite = frame
                .nodes
                .iter()
                .find(|node| {
                    node.key.starts_with(&format!("{}/", hit.key))
                        && node.dest == hit.rect
                        && node.shown(&view)
                        && matches!(&node.draw, Draw::Sprite { .. })
                })
                .expect("an actionable button must have visible art of the same size");
            assert_eq!(
                visible(hit.rect, hit.clip),
                visible(sprite.dest, sprite.clip)
            );
            let point = [
                hit.rect.x + hit.rect.w * 0.25,
                hit.rect.y + hit.rect.h * 0.25,
            ];
            let picked = hit_test(&frame.hits, point).expect("visible button point");
            assert!(picked.key == hit.key || picked.kind == json_ui::HitKind::Draggable);
        }
        let jump = control(&frame, "cinnabar.touch.jump");
        assert!(
            jump.rect.x > size[0] * 0.5,
            "right-anchored action remains on the right"
        );
        let look = control(&frame, "cinnabar.touch.look");
        assert_eq!(
            [look.rect.x, look.rect.y, look.rect.w, look.rect.h],
            [0.0, 0.0, size[0], size[1]]
        );
        assert_eq!(
            hit_test(&frame.hits, [size[0] - 1.0, size[1] * 0.5])
                .unwrap()
                .pressed,
            look.pressed
        );
        assert!(hit_test(&frame.hits, [size[0], 10.0]).is_none());
        assert!(hit_test(&frame.hits, [-1.0, 10.0]).is_none());
    }
}

#[test]
fn clipped_controls_cannot_capture_touches_outside_their_painted_portion() {
    let view = ViewState::default();
    let frame = render([160.0, 96.0], &view);
    let stick = control(&frame, "cinnabar.touch.joystick");
    assert!(
        stick.rect.y < 0.0,
        "small viewport exercises partial clipping"
    );
    let sprite = frame
        .nodes
        .iter()
        .find(|node| {
            node.key.starts_with(&format!("{}/", stick.key))
                && node.dest == stick.rect
                && node.shown(&view)
        })
        .unwrap();
    assert_eq!(
        visible(stick.rect, stick.clip),
        visible(sprite.dest, sprite.clip)
    );
    assert!(!stick.contains([stick.rect.x + stick.rect.w * 0.5, -1.0]));
    assert!(hit_test(&frame.hits, [stick.rect.x + stick.rect.w * 0.5, -1.0]).is_none());
}

#[test]
fn forward_joystick_feedback_moves_up_and_action_feedback_keeps_its_hit_rect() {
    let initial = render([320.0, 180.0], &ViewState::default());
    let knob = initial
        .hits
        .iter()
        .find(|hit| hit.kind == json_ui::HitKind::Draggable)
        .unwrap();
    let jump = control(&initial, "cinnabar.touch.jump");
    let mut view = ViewState::default();
    view.drags.insert(knob.key.clone(), [0.0, -knob.rect.h]);
    let held = render_held([320.0, 180.0], &view, &["joystick", "jump"]);
    let moved = held.hits.iter().find(|hit| hit.key == knob.key).unwrap();
    assert!(
        moved.rect.y < knob.rect.y,
        "positive forward movement raises the knob"
    );
    assert_eq!(control(&held, "cinnabar.touch.jump").rect, jump.rect);
    let icon = held
        .nodes
        .iter()
        .find(|node| {
            node.key.starts_with(&format!("{}/", jump.key))
                && node.dest == jump.rect
                && node.shown(&view)
        })
        .unwrap();
    assert!(matches!(&icon.draw, Draw::Sprite { texture, .. } if texture.ends_with("_pressed")));
}

#[test]
fn two_independent_action_fingers_both_show_pressed_art() {
    let view = ViewState::default();
    let idle = render([320.0, 180.0], &view);
    let held = render_held([320.0, 180.0], &view, &["jump", "attack"]);
    for action in ["cinnabar.touch.jump", "cinnabar.touch.attack"] {
        let hit = control(&held, action);
        assert_eq!(hit.rect, control(&idle, action).rect);
        let icons: Vec<_> = held
            .nodes
            .iter()
            .filter(|node| node.key.starts_with(&format!("{}/", hit.key)) && node.shown(&view))
            .collect();
        assert_eq!(icons.len(), 1, "each held button paints one state");
        assert!(
            matches!(&icons[0].draw, Draw::Sprite { texture, .. } if texture.ends_with("_pressed")),
            "both independent action fingers must have pressed feedback"
        );
    }
}
