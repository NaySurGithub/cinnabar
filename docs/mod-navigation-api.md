# Local terrain and automation capabilities

Three separate grants, denied by default: `terrain`, `automation`, `hotbar`.
They are available to local components and packages through the extension world.
Existing components remain compatible and receive no new authority by default.

## Terrain and block properties

`terrain.read-region(query)` takes inclusive minimum and exclusive maximum integer
coordinates in the current dimension. A query contains at most 256 cells, lies
within 128 blocks of the player's current eye on each axis, and shares a four-query
per-frame budget. Bounds, volume and distance are checked before any terrain work.

The first read returns `none` and stages a request. After a successful frame the
client samples requested regions before the next frame. Repeating a query returns
that current observation and requests another refresh. A caller must handle `none`
after changing queries, losing the session, or leaving the permitted range.
Requests from a trapped callback are discarded.

The response carries session, dimension, query, column revisions including a
neighboring halo, and every cell in x/y/z scan order. `loaded: false` is unknown,
never air. Each loaded cell retains all palette layers. Each layer carries its
runtime identifier, optional resource identifier and canonical state JSON, tags,
and optional resolved properties. Missing metadata or unavailable geometry
neighbors produce absent properties rather than approximate geometry.

Collision and selection shapes are block-local double-precision bounds, including
native shape offsets and contextual geometry. Physics facts include friction,
speed factors, fluid height, liquid and movement flags, and native surface response.
The mod owns raycasting, pathfinding, hazard policy and caching. Compare revisions
only within the same session and dimension; include all returned neighbor columns.

## Ordinary gameplay input

`automation.set-controls(some(controls))` contributes digital directions, jump,
sprint, sneak, attack and item use for the current frame. It requires the separate
`automation` grant, focused captured gameplay and no input-owning mod panel.
Physical controls remain effective. Opposing directions cancel; diagonal movement
is unit-clamped. The input domain generates press, held and release phases, and
ordinary simulation, cooldowns and interaction admission remain authoritative.

Renew every frame. Omission or `none` releases synthetic input. Menus, lost focus,
a changed session/dimension/input authority, unload and traps stop renewal. Multiple
mods resolve automation in load order: the earliest publishing instance wins.

`automation.select-hotbar(slot)` requires `hotbar`, accepts slots 0 through 8, and
queues an ordinary semantic number-key selection. Physical hotbar actions take
priority. The normal equipment queue and transport publish the selection.

No raw packets, direct velocity changes, world mutation, unrestricted file access,
container transfers, crafting or automatic chunk loading are added.

## Computation limits

Frames authorized for terrain or automation receive a bounded geometry-work fuel
budget, with the same memory limit and wall-clock backstop. Navigation work must
still be incremental. Queries are read from captured gameplay frames; screens
suspend requests. This is not an exemption from rendered-frame performance gates.
