//! x=24: server WIT 0.5's calls, which the runtime refuses until they are implemented.

use experience_sdk::server::{BlockPos, BlockState, Callback, NewStack, StateValue, WorldError};

/// Each 0.5 call made once, at the interacted block `p` and the block above it, `up`, as
/// `<call> <ok|error>` pairs.
pub(crate) fn wit_0_5_calls(ctx: &Callback, p: BlockPos, up: BlockPos) -> String {
    let stack = || NewStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 1,
        data: None,
    };
    let state = BlockState {
        name: "probe:on".to_owned(),
        value: StateValue::Bool(true),
    };
    format!(
        "block-states {} set-block-state {} network {} inventory {} set-slot {} drop-item {}",
        refusal(ctx.block_states(p)),
        refusal(ctx.set_block_state(p, &[state])),
        refusal(ctx.network()),
        refusal(ctx.inventory()),
        refusal(ctx.set_slot(0, Some(&stack()))),
        refusal(ctx.drop_item(up, &stack())),
    )
}

/// A world error's name, or `ok` for any success.
fn refusal<T>(result: Result<T, WorldError>) -> &'static str {
    result.map_or_else(|error| error.name(), |_| "ok")
}
