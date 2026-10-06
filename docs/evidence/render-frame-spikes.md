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

## Follow-up investigation

Merged `origin/dev` at `93db63d00` through merge commit `d5cd592ed`.
The material/gamma pipeline contracts from dev remain intact. The updated route
also turns toward the last gallery before its screenshot; the four flight legs
and inventory workload are unchanged. No Bevy patch or vendored dependency was added.

### Regression audit

The original branch was replayed before merging: render maximum 127.377 ms,
render p99 7.687 ms, and 363 intervals over 12.5 ms among 14,592. The old
2,036 ms maximum was not consistently reproducible. A merged baseline and an
intermediate staged candidate were then captured before the final fixes.

| Run | Render median / p99 / max (ms) | Intervals | >9.167 ms | >12.5 ms | ≥16.667 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Merged baseline | 2.061 / 18.245 / 145.185 | 13,988 | 2,898 | 565 | 255 |
| Early staged candidate | 1.576 / 4.807 / 1017.469 | 14,624 | 874 | 61 | 22 |
| Staged replay 1, before clock fix | 1.603 / 5.146 / 153.295 | 14,581 | 1,116 | 102 | 53 |
| Staged replay 2, same binary | 3.528 / 31.378 / 152.785 | 12,396 | 4,355 | 1,724 | 922 |

The final clock fix was followed by two full replays, then a repeat of the
saved merged baseline. The baseline repeat uses the same final-gallery camera
turn as the final runs; it requested one late outline key.

| Measurement | Merged baseline | Baseline replay | Final 1 | Final 2 |
| --- | ---: | ---: | ---: | ---: |
| Render median / p99 (ms) | 2.061 / 18.245 | 2.557 / 25.041 | 3.307 / 22.583 | 2.367 / 16.559 |
| Render maximum (ms) | 145.185 | 169.798 | 175.988 | 150.659 |
| Start intervals | 13,988 | 13,193 | 13,235 | 13,896 |
| Interval p99 / max (ms) | 23.765 / 411.025 | 32.183 / 173.858 | 28.727 / 194.591 | 22.998 / 165.767 |
| Intervals >9.167 ms | 2,898 | 3,149 | 4,715 | 3,365 |
| Intervals >12.5 ms | 565 | 986 | 1,340 | 790 |
| Intervals ≥16.667 ms | 255 | 646 | 741 | 288 |

These runs are diagnostic comparisons, not proof of a percentage speedup.
Other client, browser GPU and compiler activity was observed. The experiments
below used the same intermediate binary with one local switch at a time; none
of those switches remain in production code.

- Disabling application prewarm reduced that run's first render frame to
  69.268 ms, but brought back seven later specialization events. Its final
  gallery requested another key inside a 145.356 ms render frame, with a
  144.422 ms submission bracket. Overall it had 890 intervals over 12.5 ms
  among 13,709, versus 61/14,624 with prewarm in the intermediate candidate.
  This supports keeping prewarm; it does not prove the cause of the earlier
  two-second outlier. Dev's separate actor warmup remained enabled.
- Disabling only per-system timers retained broad stage tracing and work
  counters. It produced render p99/max 33.866/146.913 ms and 1,983 intervals
  over 12.5 ms among 11,786. This does not isolate instrumentation as the
  source of the large outlier or establish its normal overhead.
- The trace representation did have a concrete memory defect: every event
  reserved space for a large render-work record, even ordinary stage spans.
  Separate fixed event/work buffers reduce reserved storage from 544 MiB to
  97.25 MiB on this target. Tests bound both capacities and verify overflow
  reporting and stable storage. This fixes memory overhead, without proving
  it caused the timing regression.
- No frame-path blocking map or explicit device wait was found. Native
  sampling found drawable acquisition sleeps and substantial Metal command
  encoding/context allocation. The sampled run's largest preparation span
  was 133.916 ms, so it cannot explain the separate 995.339 ms preparation
  outlier. A serial-schedule experiment failed Metal thread-affinity checks
  before joining; it was discarded and supplies no timing evidence.

### Changes and bounded work

- Recurring UI, cloud, particle, dropped-item, block-entity, hand, lightmap,
  atmosphere, nametag and terrain-animation clock updates share four retained
  4 MiB mapped staging buffers. Copies are encoded before drawing in the main
  submission. At most 512 pending copies are
  retained. The pool never grows or waits for mapping. Exhaustion uses counted
  direct queue-write fallback, which can still allocate or stall in the driver.
  Overlapping fallback writes flush older staged copies first so newer bytes win.
- Initial/replacement neutral actor artwork and UI texture generations stay
  private until complete. Each owner issues at most 512 KiB and one texture
  allocation per preparation, totaling at most 1 MiB for these two generations
  per frame. Existing in-place UI dirty-layer transactions remain atomic and
  can exceed that cap; player skins and other texture owners are not covered.
  Cancelled UI generations release private storage; changed actor lifetimes clear
  old draws. Pending artwork retains the previous complete actor frame, including
  its pose and skins. Pending UI retains one coherent publication while newer
  compatible revisions wait; these reload behaviors still need live qualification.
- An intermediate trace isolated a remaining 38.139 ms hand preparation span
  inside buffer uploads. Hand pose uploads now share staging; unchanged hand
  projection/light uniforms do no work. Changed world lighting stages one 4 KiB
  table. The merged baseline also showed 20.163 ms atmosphere and 7.409 ms
  nametag preparation spans; these now stage changed data and skip unchanged
  uploads. Nametag records remain capped at 81,920 bytes. Geometry, skin and
  other nonparticipating uploads retain their paths. A valid staged replay then
  exposed a 10.591 ms direct terrain-clock write inside a 39.143 ms render frame;
  clock changes now stage exactly 16 bytes and unchanged clocks do no work.
- The corrected final-gallery camera exposed a missing block-selection outline
  prewarm variant. The valid repeated baseline requested it at frame 13,054;
  both final runs requested it during loading and no keys later. Its real-cache
  regression failed with the old list and passed with the fix for every MSAA/HDR
  combination. The repeated baseline's late-key frame was only 4.250 ms, so a
  specialization request is not by itself proof of a hitch.
- Actor, viewmodel and terrain completion callbacks attach to the submitted
  frame; they no longer create empty submission sentinels. All our readback
  rings share one nonblocking completion poll. Pass-only GPU timestamp resolve
  and mapping share the main encoder; per-draw profiling on capable adapters
  still adds one counted resolve submission after deferred draw generation.
- A direct encoder-timestamp experiment corrupted rendering: three discovery
  replays captured black frames, and a same-binary on/off replay lost water,
  items, text, HUD and hand only with direct markers enabled. It was dropped.
  Isolated compute markers remain, counted as `timestamp_marker_passes`; their
  CPU encoding cost remains part of the diagnostic workload. Native backend
  encoders are not counted by this field. The invalid replays are excluded
  from before/after performance comparisons.
- Queue submissions are counted at our call sites, including arena migration.
  Stock Bevy submits the main graph once; its backend presentation work is
  outside this counter. The new graph bracket includes schedule delay and may
  include private pipeline processing, so it is not an exact native submit timer.
  Hidden mode requests AutoNoVsync, resolving to Immediate on Metal; drawable
  acquisition can still wait. No presentation-policy change was made.

The two valid staged replays before the final clock change used the same binary,
yet recorded 102 and 1,724 intervals over 12.5 ms. Both remain reported; replay 2
also overlapped offline trace analysis. Final replays below ran without our
builds, tests or trace analysis alongside them. All 81
application pipeline keys were requested in their first frame, with no later
requests. The trace pools did not truncate, the upload pool stayed at 16 MiB,
and neither run used upload fallback or a blocking poll.

In replay 1, a 98.922 ms flight frame had long manage/queue/preparation brackets
but its largest measured application system span was only 0.373 ms. Another
39.143 ms frame contained the 10.591 ms terrain-clock write and a 9.905 ms UI
node span. The first frame still took 153.295 ms, including 130.319 ms in the
preparation bracket; measured inner spans did not account for that entire gap.
Replay 2 retained a 152.785 ms first frame and a 125.647 ms flight frame.
These gaps are unresolved; the data does not justify assigning them entirely
to another process or the driver.

Final replay 1/2 each allocated only four staging buffers (16 MiB total),
used zero staging fallbacks, and recorded one nonblocking poll per render frame
with zero explicit waits. They staged 140,723/147,021 writes and
128,086,888/127,587,160 bytes. Both requested 81 application keys in frame 1
and zero later. Each recorded 45 extra submissions, all matching arena migrations:
replay 1 had zero extras in 13,213/13,234 frames (maximum 11); replay 2 had
zero extras in 13,873/13,895 frames (maximum 8). Stock main-graph submission
is additional. Roughly 20 isolated timestamp marker passes remain per frame.
The baseline predates the extra-submission and staging counters; absent fields
must not be interpreted as zero actual work. Its existing poll counter records
five polls per render frame.

The terrain-clock preparation maximum fell from 10.112 ms in the repeated
baseline (and 19.932 ms in the earlier staged replay) to 0.155/3.856 ms in
final replay 1/2. Nametag preparation maxima were 10.127 ms in the repeated
baseline and 0.248/0.241 ms finally. These observed spans support the targeted
changes, without proving that unrelated frame gaps improved. Startup UI still
reached 15.311/20.329 ms, and hand preparation 23.315/12.173 ms. Bounding work
and upload bytes has not bounded driver or scheduler latency.

The final first frames still took 175.988/150.659 ms. A 120.716 ms flight frame
had a largest measured application-system span of 0.733 ms; a 118.959 ms
inventory frame spent 111.414 ms in the queue bracket with short measured
application systems. A final replay without aggregate profiling and without
frame tracing rendered correctly but its last printed diagnostic, at frame
10,956, already counted 1,925 hitches and 1,104 hard hitches. That is a partial
cumulative diagnostic, not a complete trace denominator. Ordinary slow-frame
monitoring and GPU timestamp markers remain active in that mode.

GPU elapsed-time aggregate counts stayed zero in these Metal captures despite
recorded timestamp marker passes. CPU spans and deterministic work counters
remain useful, but these captures do not establish GPU execution durations.

### Verification and remaining work

- Compiled `render` and `bedrock-client` test targets with
  `developer-control,enhanced`, then rebuilt the developer-control client. All
  Cargo checks, builds and focused tests used the shared build-slot limiter.
- Passing focused regressions cover actor generation bounds/lifetime invalidation,
  UI generation publication/cancellation and ordered dirty writes, unchanged
  actor/UI/cloud/hand/light/atmosphere/nametag work, staging capacity/recycling/
  overlap ordering/metadata exhaustion/failed-slot quarantine, and real terrain
  texture/clock identities and exact changed-clock bytes.
- GPU timing tests cover empty-frame submission counts, async readback and deferred
  per-draw encoding. Completion tests cover existing-submission callbacks; trace
  tests cover fixed memory, overflow and stable storage. The real outline-cache
  regression was run failing with the old warmup list and passing with the fix.
- The UI integration suite passes (72 tests; two pre-existing ignored benchmarks).
  The previously failing two-phase cull graph regression passes. The old source-
  spelling upload assertion was replaced with a production resource/work test.
- Touched-file formatting, diff checks and production/module-root line limits pass.
  Linux/Windows integration, full CI and review remain the remote gate. No full
  workspace, clippy, nextest or verify-affected sweep was run locally.

The 1.5-refresh-interval hitch target is **not met**. The earlier two-second
regression is not fully attributed; there is no claim that all remaining stalls
come from the OS or another process. The release/displayed-frame, 128 blocks/s,
animated-mob, skin-change, media/mod/enhanced and multiplayer gates remain open.
Raw traces, sample output, screenshots and harnesses remain in ignored local
storage. Historical results below are retained for comparison.

## Previous round measurements

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
