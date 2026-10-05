# Render-frame spike investigation

Diagnostic captures on the owner’s Apple M3 Pro, macOS 26.5.1, Metal,
1920×1080 at scale 1, 12-chunk radius and a 120 FPS cap, on 2026-10-06.
All captures use stock Bevy and an optimized development build with
`developer-control`. Hidden start-to-start intervals are not displayed frame
intervals or release qualification. Driver caches and other applications were
not controlled. Another client and compiler processes were observed during the
investigation; these observations do not prove which stalls they caused.

The local-only MCP route joins a fresh flat world, introduces ten block
materials, item/text entities and eleven particle event types, opens/closes
inventory, walks, then flies four roughly 1,700-block legs at about 65 blocks/s.
The route ends with another gallery in the newly visited area. It does not
exercise animated mobs, player skin changes, media screens, mods, enhanced
shadows, multiplayer load or the required 128 blocks/s qualification replay.
Fresh gallery and inventory images were inspected for geometry, materials,
text, layering and clipping. Existing incomplete creative icons remain.
No native windows were shown and no OS input was used.

## Measurements

The baseline restores the old actor readiness predicate, misses the UI binding
cache, recreates cloud storage and disables the new application prewarm gate.
All other instrumentation and the native hidden-mode fix are shared. The final
candidate adds nested GPU API timers; it has the same rendering fixes as
candidate 1. Traces are complete, without capacity truncation.

| Measurement | Stock baseline | Candidate 1 | Final candidate |
| --- | ---: | ---: | ---: |
| Render frames | 14,689 | 13,563 | 14,644 |
| Render median | 1.512 ms | 1.964 ms | 1.420 ms |
| Render p99 | 2.300 ms | 25.025 ms | 3.898 ms |
| Render maximum, including startup | 51.580 ms | 1,302.221 ms | 2,036.373 ms |
| Start intervals | 14,690 | 13,564 | 14,645 |
| Interval median | 8.529 ms | 8.421 ms | 8.403 ms |
| Interval p99 | 9.647 ms | 29.621 ms | 10.423 ms |
| Intervals over 9.167 ms | 563 (3.83%) | 2,309 (17.02%) | 771 (5.26%) |
| Intervals over 12.5 ms | 16 | 725 | 62 |
| Intervals at least 16.667 ms | 9 | 420 | 34 |
| Main CPU median / p99 | 2.123 / 9.227 ms | 3.381 / 14.974 ms | 2.069 / 8.711 ms |
| Surface preparation median | 6.952 ms | 5.960 ms | 6.998 ms |

The final candidate reduces median render time but does **not** establish an
improvement in hitches or tail latency. After the first 20 trace seconds,
baseline render p99/max were 2.149/19.843 ms, with six render frames over
12.5 ms among 12,331. The final candidate had 3.780/25.849 ms and eight among
12,577. The poorer candidate 1 remains reported rather than discarded.

## Findings and changes

- Empty actor frames were never considered to have resident player skins.
  Actor preparation invalidated its retained artwork bindings every frame,
  causing 22 new bind groups with no actor instances. Empty frames now complete
  synchronization; actual skin residency still gates nonempty frames.
- The retained UI composite created one bind group each frame. Its cache now
  retains the two scene ping-pong bindings by resource identity and evicts
  replaced targets with fixed storage. Together these changes reduce the
  stationary gallery from exactly 23 groups/frame to zero. Every confidently
  classified flight frame in the final capture also created zero groups.
- `cloud_render_prepare_cloud_records` took 18.258 ms inside a 19.843 ms
  baseline flight frame. Moving its cloud window recreated GPU storage and
  bindings, rescanned pack occupancy, and formatted/logged evidence. Storage is
  now capacity-reused, uploads contain only live records, and source diagnostics
  are cached. The final maximum observed cloud span was 0.563 ms. The earlier
  combined timer cannot prove which removed operation caused the 18 ms sample.
- The baseline 51.580 ms frame requested eight new application pipeline keys;
  its submission bracket took 49.895 ms. Built-in application variants are now
  enumerated through their real caches at load, and world presentation waits
  for compilation plus a later GPU frame. The final trace requested all 52 keys
  initially and zero thereafter. No steady-state key churn was observed even
  in the baseline. Private stock FXAA variants are not included in our warmup.
- Startup remains a serious hitch: final actor/UI preparation took
  335.172/308.789 ms; nested texture creation, texture upload and direct compute
  creation accumulated 395.209/293.288/273.329 ms. These overlapping call totals
  do not account for the entire 2,036.373 ms frame. Prewarming moves work to
  loading; it does not make loading hitch-free.
- In the final stationary gallery, dropped-item, block-entity and UI preparation
  took 14.920/10.076/10.061 ms during one 17.021 ms render frame. Nested buffer
  uploads accumulated 35.213 ms across concurrent calls. This localizes part of
  the stall to upload calls, without proving driver execution versus scheduling.
  Other 48.901/40.269 ms render frames had short application spans and zero new
  keys/groups; submission brackets were 32.759/39.253 ms. Those remain unresolved.

The final trace has 3,219 slow markers per 14,645 intervals (21.98%), but only
771 intervals exceed 9.167 ms. Most markers instead violate the separate 4 ms
main budget: 2,405 are main-only and another 34 are main-plus-render. Baseline
has 2,533 markers (17.24%) and 1,962 main-only markers. This distinguishes the
observed approximately 22% diagnostic count from an interval failure rate;
it does not establish that the owner's original capture has the same cause.

For intervals strictly between 9.167 and 12.5 ms, baseline/final counts are
547/709, interval medians 9.371/9.403 ms, and surface medians 7.134/7.131 ms.
Surface preparation exceeds the paired render span in 99.45%/96.76% of those
samples. It includes scheduling and overlaps main-thread work; this does not
prove a displayed drawable-wait cause. No individual application render system
dominates that population. For fast intervals that breach the main budget,
`world_poll` is the largest broad main stage (2.481 ms average in the final
capture), but these measurements do not identify a specific fix inside it.
All three captures used five nonblocking
readback polls per frame and zero explicit waits. Upload totals differ with
startup and workload evolution; they are not a controlled throughput comparison.

## Verification and remaining gate

Focused actor, cloud, UI binding, warmup and profiler tests passed (49 tests),
plus the loading-gate regression. The actor regression fails with the old empty
frame predicate restored. Assertions cover real buffer/bind-group identity,
bounded upload bytes, zero unchanged work and zero repeated specialization;
none assert milliseconds. Touched render, client-ui and app test targets compile,
including enhanced rendering; the hidden developer client builds.

Our macOS application subclass is installed only for hidden developer mode,
before the window backend. The post-launch native witness reports prohibited
activation, inactive application, no main menu and no services menu. Normal
window policy is unchanged; its deterministic tests compile. A visible launch
was intentionally not attempted.

All application extract, recurring render systems and custom graph nodes have
named timing; GPU API spans are nested. The slow line and trace retain the top
12 and own-layer upload, binding, specialization, arena and readback counts.
Stock pipeline processing remains inside the submission bracket, so native
compilation is not independently measured. Instrumentation and these fixes
leave the performance gate open: startup, residual submission/upload stalls,
chronic displayed-frame overruns and version-matched visual parity need further
qualification. Captures and harness artifacts remain in ignored local storage
under `own-baseline-02`, `own-candidate-01` and `own-candidate-02`.
