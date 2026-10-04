//! Server WIT 0.5's calls. x=24 makes each once: the block-state calls, which the runtime
//! implements, and the rest, which it refuses until they are implemented. x=25 toggles a lamp.

use experience_sdk::server::{
    BlockDef, BlockPos, BlockState, BlockType, Callback, Mining, NewStack, PlacementStates,
    Registration, StateDef, StateValue, StateValues, TextureBinding, WorldError, cube,
};

/// A cube with the facing placement trait and one bool state, [`LAMP_ON`].
const LAMP: &str = "probe:lamp";
pub(crate) const LAMP_ON: &str = "probe:on";

/// The probe's blocks, each a cube showing `counter.png`: the counter `counter`, without states,
/// and the lamp, with the facing trait and [`LAMP_ON`]; and no items.
pub(crate) fn registration(counter: &str) -> Registration {
    let def = |id: &str, display_name: &str| BlockDef {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        textures: vec![TextureBinding {
            slot: "*".to_owned(),
            path: "counter.png".to_owned(),
        }],
        mining: Mining::Breakable(1.0),
    };
    let lamp = BlockType {
        states: vec![StateDef {
            name: LAMP_ON.to_owned(),
            values: StateValues::Bool,
        }],
        placement: PlacementStates::FACING_DIRECTION,
        ..cube(def(LAMP, "Probe Lamp"))
    };
    Registration {
        blocks: vec![cube(def(counter, "Probe Counter")), lamp],
        items: Vec::new(),
    }
}

/// Places a lamp at `up`, the block above the interacted block `p`, reads its states, lights it
/// and reads them again, then makes each other 0.5 call once, as `<call> <states|ok|error>`
/// pairs short enough for one tell.
pub(crate) fn wit_0_5_calls(ctx: &Callback, p: BlockPos, up: BlockPos) -> String {
    let stack = || NewStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 1,
        data: None,
    };
    // Its outcome shows in the states read after it.
    let _ = ctx.set_block(up, LAMP);
    let before = states(ctx.block_states(up));
    let lit = refusal(ctx.set_block_state(up, &[on(true)]));
    let after = states(ctx.block_states(up));
    format!(
        "states {before} set {lit} states {after} network {} inventory {} set-slot {} \
         drop-item {}",
        refusal(ctx.network()),
        refusal(ctx.inventory()),
        refusal(ctx.set_slot(0, Some(&stack()))),
        refusal(ctx.drop_item(p, &stack())),
    )
}

/// Turns the lamp at `p` on or off, keeping its data, and tells its states.
pub(crate) fn toggle_lamp(ctx: &Callback, p: BlockPos) -> String {
    let lit = ctx.block_states(p).map(|states| {
        states
            .iter()
            .any(|state| state.name == LAMP_ON && matches!(state.value, StateValue::Bool(true)))
    });
    match lit {
        Ok(lit) => match ctx.set_block_state(p, &[on(!lit)]) {
            Ok(()) => format!("lamp {}", states(ctx.block_states(p))),
            Err(error) => format!("error {}", error.name()),
        },
        Err(error) => format!("error {}", error.name()),
    }
}

fn on(lit: bool) -> BlockState {
    BlockState {
        name: LAMP_ON.to_owned(),
        value: StateValue::Bool(lit),
    }
}

/// States as `name=value` joined by `,`, or the error's name.
fn states(result: Result<Vec<BlockState>, WorldError>) -> String {
    match result {
        Ok(states) => states
            .iter()
            .map(|state| match &state.value {
                StateValue::Bool(value) => format!("{}={value}", state.name),
                StateValue::Choice(value) => format!("{}={value}", state.name),
            })
            .collect::<Vec<_>>()
            .join(","),
        Err(error) => error.name().to_owned(),
    }
}

/// A world error's name, or `ok` for any success.
fn refusal<T>(result: Result<T, WorldError>) -> &'static str {
    result.map_or_else(|error| error.name(), |_| "ok")
}
