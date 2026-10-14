//! Guest component that reads one terrain region per frame through the real `extension` world.

const SESSION_WIT: &str = include_str!("../../../experience-sdk/wit/session/session.wit");
const CAPABILITIES_WIT: &str =
    include_str!("../../../experience-sdk/wit/client/deps/server-experience/capabilities.wit");
const EXTENSION_WIT: &str = include_str!("../../../mod-api/wit/extension.wit");
/// Linear-memory offset of the canonical-ABI return area; guest allocations start above it.
const RETURN_AREA: u32 = 0;
const HEAP: u32 = 1024;

/// Returns a component whose frame reads `[min, max)` and then runs `check`.
/// `check` sees the result tag at offset 0 and, for `ok`, the option tag at offset 8.
pub fn terrain_guest(min: [i32; 3], max: [i32; 3], check: &str) -> Vec<u8> {
    let package = EXTENSION_WIT
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("package "))
        .and_then(|line| line.strip_suffix(';'))
        .expect("extension WIT starts with its package");
    let (name, version) = package.split_once('@').expect("versioned package");
    let arguments = min
        .iter()
        .chain(&max)
        .map(|value| format!("i32.const {value}"))
        .collect::<Vec<_>>()
        .join(" ");
    let module = format!(
        r#"(module
  (import "{name}/terrain@{version}" "read-region" (func $read (param i32 i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 64)
  (global $next (mut i32) (i32.const {HEAP}))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $at i32)
    global.get $next local.get 2 i32.add i32.const 1 i32.sub
    i32.const 0 local.get 2 i32.sub i32.and local.tee $at
    local.get 3 i32.add global.set $next local.get $at)
  (func (export "init"))
  (func (export "frame")
    i32.const {HEAP} global.set $next
    {arguments} i32.const {RETURN_AREA} call $read
    {check}))"#
    );
    let mut module = wat::parse_str(module).expect("terrain guest WAT");
    let mut resolve = wit_parser::Resolve::default();
    resolve.push_str("session.wit", SESSION_WIT).unwrap();
    resolve
        .push_str("capabilities.wit", CAPABILITIES_WIT)
        .unwrap();
    let extension = resolve.push_str("extension.wit", EXTENSION_WIT).unwrap();
    let world = resolve
        .select_world(&[extension], Some("extension"))
        .unwrap();
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .unwrap();
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}

/// Traps unless the read succeeded and returned (`true`) or withheld (`false`) a region.
pub fn expect_region(present: bool) -> String {
    format!(
        "i32.const {RETURN_AREA} i32.load8_u if unreachable end \
         i32.const {} i32.load8_u i32.const {} i32.ne if unreachable end",
        RETURN_AREA + 8,
        u8::from(present)
    )
}
