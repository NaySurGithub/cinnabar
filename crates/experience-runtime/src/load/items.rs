//! Server WIT 0.5's items: an Experience's own, which `register` declares, and the server's,
//! which the adapter lists at load.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

use super::{asset_path, check_display_name, is_block_name};
use crate::host::cinnabar::experience_server::types as wit;
use crate::limits::{MAX_BLOCK_NAME_BYTES, MAX_ITEMS, MAX_SERVER_ITEMS, MAX_STACK_SIZE};
use crate::protocol::{BlockDef, ItemDef, ServerItem};

/// Checks the items an Experience declares: at most [`MAX_ITEMS`], each `<id>:<name>` like a
/// block and distinct from every block, with a display name like a block's, an indexed icon and
/// 1 to [`MAX_STACK_SIZE`] to a stack. Icon paths become absolute.
pub(super) fn validate_items(
    root: &Path,
    files: &BTreeMap<String, String>,
    id: &str,
    blocks: &[BlockDef],
    items: Vec<wit::ItemDef>,
) -> Result<Vec<ItemDef>> {
    ensure!(
        items.len() <= MAX_ITEMS,
        "register declared {} items; the limit is {MAX_ITEMS}",
        items.len()
    );
    let namespace = format!("{id}:");
    let mut checked: Vec<ItemDef> = Vec::with_capacity(items.len());
    for wit::ItemDef {
        id,
        display_name,
        icon,
        max_stack,
    } in items
    {
        let Some(name) = id.strip_prefix(&namespace) else {
            bail!("item \"{id}\" is outside namespace \"{namespace}\"");
        };
        ensure!(
            is_block_name(name),
            "item \"{id}\" has an invalid name: the part after \"{namespace}\" must match \
             ^[a-z0-9_]{{1,{MAX_BLOCK_NAME_BYTES}}}$"
        );
        ensure!(
            checked.iter().all(|item| item.id != id),
            "item \"{id}\" is declared twice"
        );
        ensure!(
            blocks.iter().all(|block| block.id != id),
            "item \"{id}\" is also a block, whose item it would replace"
        );
        let context = || format!("item \"{id}\"");
        check_display_name(&display_name).with_context(context)?;
        let icon = asset_path(root, files, &icon)
            .context("icon")
            .with_context(context)?;
        ensure!(
            (1..=MAX_STACK_SIZE).contains(&max_stack),
            "item \"{id}\" stacks to {max_stack}; it needs 1 to {MAX_STACK_SIZE}"
        );
        checked.push(ItemDef {
            id,
            display_name,
            icon,
            max_stack,
        });
    }
    Ok(checked)
}

/// The server's items by id, as the adapter lists them: at most [`MAX_SERVER_ITEMS`], each once,
/// 1 to [`MAX_STACK_SIZE`] to a stack.
pub(super) fn server_items(items: Vec<ServerItem>) -> Result<HashMap<String, u8>> {
    ensure!(
        items.len() <= MAX_SERVER_ITEMS,
        "the server lists {} items; the limit is {MAX_SERVER_ITEMS}",
        items.len()
    );
    let mut table = HashMap::with_capacity(items.len());
    for ServerItem { id, max_count } in items {
        ensure!(
            (1..=MAX_STACK_SIZE).contains(&max_count),
            "server item \"{id}\" stacks to {max_count}; it needs 1 to {MAX_STACK_SIZE}"
        );
        ensure!(
            table.insert(id.clone(), max_count).is_none(),
            "server item \"{id}\" is listed twice"
        );
    }
    Ok(table)
}
