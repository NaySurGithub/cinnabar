use super::*;
use ui::{UiNode, UiNodeId, UiRect, UiVisual};

fn rect(x: f32, y: f32, w: f32, h: f32) -> UiRect {
    UiRect::new(
        ui::UiPoint::new(x, y).unwrap(),
        ui::UiPoint::new(x + w, y + h).unwrap(),
    )
    .unwrap()
}

fn group(id: u32, x: f32, y: f32, w: f32, h: f32) -> UiNode {
    UiNode::new(UiNodeId::new(id), None, rect(x, y, w, h)).with_clip_children(true)
}

fn leaf(id: u32, parent: u32, x: f32, y: f32, w: f32, h: f32) -> UiNode {
    UiNode::new(
        UiNodeId::new(id),
        Some(UiNodeId::new(parent)),
        rect(x, y, w, h),
    )
    .with_visual(UiVisual::Solid {
        texture_page: 0,
        color: [1, 2, 3, 255],
    })
}

fn ids(nodes: &[UiNode]) -> Vec<u32> {
    nodes.iter().map(|node| node.id().get()).collect()
}

#[test]
fn the_gui_rect_is_the_panel_and_every_control_but_full_screen_ones() {
    let root = [480.0, 270.0];
    let panel = Some([152.0, 52.0, 176.0, 166.0]);
    // A recipe book beside the panel and a full-screen dismiss region.
    let controls = [[4.0, 52.0, 147.0, 166.0], [0.0, 0.0, 480.0, 270.0]];
    let gui = gui_rect(panel, controls.iter().copied(), root).unwrap();
    assert_eq!(
        [gui.x, gui.y, gui.width, gui.height],
        [4.0, 52.0, 324.0, 166.0]
    );
    assert!(gui_rect(None, std::iter::empty(), root).is_none());
}

#[test]
fn overlay_nodes_touching_a_forbidden_area_are_dropped() {
    let mut nodes = vec![
        group(1, 0.0, 0.0, 400.0, 300.0),
        // Vanilla's nodes before the overlay are never touched.
        leaf(2, 1, 100.0, 100.0, 10.0, 10.0),
        group(3, 100.0, 0.0, 300.0, 300.0),
        // Relative to its group: at 110..120, inside the forbidden 100..200.
        leaf(4, 3, 10.0, 10.0, 10.0, 10.0),
        // At 300..320, clear of it.
        leaf(5, 3, 200.0, 10.0, 20.0, 10.0),
    ];
    clip_overlay(&mut nodes, 2, &[[100.0, 0.0, 200.0, 300.0]]);
    assert_eq!(ids(&nodes), [1, 2, 3, 5]);
}

#[test]
fn lifted_nodes_move_above_later_ones_under_a_copy_of_their_group() {
    let mut nodes = vec![
        group(1, 0.0, 0.0, 400.0, 300.0),
        leaf(2, 1, 0.0, 0.0, 10.0, 10.0),
        // The held stack, in the shared group.
        leaf(3, 1, 50.0, 50.0, 16.0, 16.0),
        leaf(4, 1, 5.0, 5.0, 10.0, 10.0),
        // A tooltip with its own group.
        group(5, 60.0, 60.0, 100.0, 20.0),
        leaf(6, 5, 0.0, 0.0, 100.0, 20.0),
    ];
    let lifted = take_lifted(&mut nodes, &[2..3, 4..6]);
    assert_eq!(ids(&nodes), [1, 2, 4]);
    // The overlay draws here, then the lifted nodes come back with fresh ids.
    let mut next = 10;
    restore_lifted(&mut nodes, lifted, &mut next);
    assert_eq!(ids(&nodes), [1, 2, 4, 10, 11, 12, 13]);
    assert_eq!(next, 14);
    // The held stack sits under a copy of its group.
    assert_eq!(nodes[3].parent(), None);
    assert!(nodes[3].clips_children());
    assert_eq!(nodes[3].bounds(), nodes[0].bounds());
    assert_eq!(nodes[4].parent(), Some(UiNodeId::new(10)));
    assert_eq!(nodes[4].bounds(), rect(50.0, 50.0, 16.0, 16.0));
    // The tooltip's own group moved with it.
    assert_eq!(nodes[5].bounds(), rect(60.0, 60.0, 100.0, 20.0));
    assert_eq!(nodes[6].parent(), Some(UiNodeId::new(12)));
    ui::UiTree::new(nodes).unwrap();
}

#[test]
fn hits_inside_a_forbidden_area_are_not_the_overlays() {
    let layout = ScreenLayout {
        screen: "crafting.inventory_screen".into(),
        size: server_experience::screen::GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: server_experience::screen::Rect {
            x: 152.0,
            y: 52.0,
            width: 176.0,
            height: 166.0,
        },
        exclusions: vec![server_experience::screen::Rect {
            x: 400.0,
            y: 0.0,
            width: 80.0,
            height: 30.0,
        }],
    };
    assert!(overlay_hit_allowed(&layout, [340.0, 60.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [320.0, 60.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [420.0, 20.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [0.0, 0.0, 480.0, 270.0]));
}
