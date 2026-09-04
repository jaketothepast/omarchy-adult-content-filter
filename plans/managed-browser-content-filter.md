# Plan: Adult-content filtering browser for Omarchy

## Problem

Hostname filters are useful but cannot inspect the URL path or content of an HTTPS page. A separate pre-rendering proxy would need to reproduce the browser's authenticated session, execute untrusted pages twice, and delay every navigation by roughly another page load. The Omarchy Adult Content Filter began as a Kids experiment answering a narrower question first: can stock Chromium pause the exact image responses it will display, classify them locally in Rust, and substitute a safe response before the original pixels become visible?

This repository is an independent experiment and eventual product integration point. It layers onto an ordinary Omarchy checkout rather than forking Chromium or replacing Omarchy's installer. The work is intentionally structured so successful generic improvements can later become focused pull requests to Omarchy, Omarchy ISO, or their package repository.

## First milestone

The first milestone is a host-side pipeline proof, not a pornography-classification accuracy claim. A successful run must:

1. launch the system's stock headed Chromium with a disposable profile;
2. load a controlled local page behind an opaque cover;
3. pause image responses before they reach the renderer;
4. pass each image through a real, warm ONNX model from Rust;
5. release normal fixtures and replace one deterministically designated fixture;
6. reveal the page only after its initial image set has reached a verdict;
7. emit machine-readable latency and outcome metrics; and
8. shut down Chromium and remove all disposable state.

The deterministic fixture override proves the complete block/substitution path without storing explicit test imagery and without conflating plumbing success with model accuracy. The model still runs for every fixture so inference timing is real.

## Non-goals for the first milestone

- Browsing or collecting live explicit content.
- Claiming a false-positive or recall rate.
- Filtering browsers other than the supervised Chromium process.
- Intercepting native applications or arbitrary device traffic.
- Shipping a trusted HTTPS interception certificate.
- Building a Chromium fork.
- Solving video, canvas, WebGL, `data:` URLs, `blob:` URLs, service workers, or browser back/forward cache in the first slice.
- Producing a hermetic ISO as a Nix derivation.

## Repository shape

```text
omarchy-kids/
├── Cargo.toml
├── crates/
│   └── browser-filter/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── browser.rs
│       │   ├── inference.rs
│       │   ├── metrics.rs
│       │   ├── policy.rs
│       │   └── main.rs
│       └── tests/
├── browser-extension/
│   ├── manifest.json
│   └── cover.css
├── fixtures/
│   └── browser-filter/
├── nix/
│   └── apps.nix
├── plans/
│   └── managed-browser-content-filter.md
├── flake.nix
├── flake.lock
└── README.md
```

The Rust crate owns only the experimental browser process, fixture server, inference session, policy decision, and metrics. The extension owns only the pre-paint cover and visible result state. Model weights and generated browser profiles are not committed.

Sibling repositories have explicit roles:

- `../omarchy` is the locally forked Omarchy runtime source.
- `../omarchy-iso` contains the existing Docker and ArchISO build/test harness.
- `../omarchy-pkgs` contains the Arch package recipes used by local-source ISO builds.

They remain independent Git repositories, not submodules. Experiment commands accept environment overrides for all three paths and otherwise use these sibling defaults.

## Components

### Rust supervisor

The supervisor uses `chromiumoxide` 0.9.1 to start an in-process loopback HTTP fixture server, create a unique temporary Chromium profile, launch headed Chromium, attach through the Chrome DevTools Protocol, and subscribe to new page targets. The experiment may use an ephemeral loopback debugging endpoint because it has a disposable profile and no private browsing data. A production implementation must use a private inherited pipe or an equivalently confined transport so another process running as the child cannot acquire browser-equivalent control. Chromium is always launched with its sandbox enabled, the cache initially disabled for deterministic tests, and the exact executable selected through `CHROMIUM_BIN`.

The supervisor enables [Fetch-domain](https://chromedevtools.github.io/devtools-protocol/tot/Fetch/) interception at the response stage for image resources. Each pause is resolved exactly once. Successful image bodies are obtained with `Fetch.getResponseBody`, bounded before decode, and passed to an inference worker through a bounded queue. Redirects, 204/304 responses, and response-error events continue without an invalid body read. An approved image uses argument-free `Fetch.continueResponse` on the tested Chromium build so the network stack retains the original response; this command is still protocol-experimental and therefore receives an explicit compatibility test. A blocked image is returned with `Fetch.fulfillRequest` as a base64-encoded local PNG carrying only `Content-Type` and `Cache-Control: no-store` headers.

ONNX work runs in `tokio::task::spawn_blocking`, never on Chromiumoxide's asynchronous handler. Body acquisition and inference have deadlines so a failed worker cannot leave the browser permanently paused.

### Rust inference engine

The inference component pins `ort` 2.0.0-rc.13 with default features disabled and `std`, `load-dynamic`, and `api-27` enabled. It dynamically loads Nixpkgs' ONNX Runtime 1.27.1 through `ORT_DYLIB_PATH` and starts with the CPU execution provider. One inference worker owns one warm session because `Session::run` takes mutable access; extra sessions and hardware execution providers wait for measurement. Image decoding, orientation normalization, channel conversion, resize/letterboxing, tensor normalization, inference, and output decoding are timed separately.

The initial model is NudeNet's 320×320 YOLOv8n ONNX weight at commit `6ccc81c6c305cccfd46d92b414f8a5c0a816574d`, pinned by SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`. It accepts float32 NCHW tensors named `images` and returns `output0` shaped `[batch, 22, 2100]`: four box channels and 18 class-score channels. The reference path pads the right and bottom to a square, resizes to 320×320, normalizes to 0–1, and appears to produce BGR tensors after two channel swaps. The experiment initially reproduces that behavior with a colored-pixel regression test and records the ambiguity for later RGB comparison.

The model is only a performance and integration proxy, not a production model decision. The upstream repository and packaged metadata disagree between AGPL-3.0 and MIT, and the weights have no sufficiently clear separate license or training-data provenance. The Nix fetch expression records the source, checksum, input contract, class mapping, and postprocessing thresholds, but the model cannot be redistributed as a product until licensing and provenance are resolved. GPU or NPU execution providers are deferred until CPU measurements fail the latency target.

### Policy

The first policy has two inputs:

- the model's already-thresholded detections; and
- a deterministic marker belonging only to the controlled flagged fixture.

All images execute the same decode and inference path. The narrow experiment policy replaces detections whose exact class is `BUTTOCKS_EXPOSED`, `FEMALE_BREAST_EXPOSED`, `FEMALE_GENITALIA_EXPOSED`, `ANUS_EXPOSED`, or `MALE_GENITALIA_EXPOSED`; covered and ambiguous classes remain allowed. The exact `http://127.0.0.1` flagged-fixture marker takes precedence with its separate deterministic reason so the harmless experiment always exercises both release and replacement behavior. This explicit class set is plumbing behavior, not a claim about accuracy or a production policy. Production page aggregation, age tiers, contextual classification, and parent overrides remain later milestones.

### Page cover

A Manifest V3 extension loaded only into the disposable profile injects declarative CSS at `document_start`, hiding document children until the root has a readiness attribute. After every initial fixture image has been resolved, the supervisor sets that attribute through CDP for an allowed page or leaves the cover in place and shows a local block state. The page never receives model details or privileged control messages.

The readiness attribute is deliberately only a pipeline-spike mechanism: hostile page JavaScript could set it, and extension CSS cannot cover Chromium-internal pages. A production cover must be extension-owned and tamper-resistant. The experiment records screenshots repeatedly while one fixture is paused and after the reveal point. These demonstrate the controlled sequence but do not prove that every Chromium cache and navigation path is flash-free; those paths have explicit later acceptance gates.

### Metrics

The headed browser writes privacy-safe per-stage, per-image `MetricRecord` JSONL to stderr. Each record contains exactly `stage`, `verdict`, `fixture_index`, and `elapsed_micros`; it contains no URL, path, response body, image bytes, tensor, cookie, or page body. With `run --json`, one separate headed summary is written to stdout containing Chromium version, intercepted/continued/replaced/unresolved counts, clean-shutdown and reveal results, DOM fixture metadata, and optional no-flash assertion evidence. Without `--json`, stdout contains a short human-readable count summary.

With `bench --json`, stdout contains four JSONL workload summaries. Each includes CPU model, ONNX Runtime version, model checksum, build mode, image count, warmup and iteration counts, median encoded bytes, and p50/p90/p95 objects for decode, preprocessing, inference, postprocessing, and total workload time. Rich run identifiers, cache outcomes, response-byte fields, pause-to-fulfill timing, cover-duration breakdowns, and memory measurements are target telemetry for a later milestone; the current implementation does not produce them.

## Data flow

```text
local fixture request
        ↓
Chromium performs normal fetch
        ↓
Fetch.response-stage pause
        ↓
Rust reads response body
        ↓
decode → resize → tensor → ONNX inference
        ↓
model result + deterministic fixture policy
        ↓
allow: continue original response    block: fulfill placeholder body
        ↓
initial image set complete
        ↓
extension reveals allowed page or shows block state
```

There is no DNS proxy and no duplicate renderer in this path. DNS reputation can become a cheap first stage later, but content inspection happens on the exact response Chromium is using after TLS handling and before display.

## Failure behavior

The experiment fails closed: if the controller detaches, the model fails, an image cannot be decoded, or the decision deadline expires, the cover remains and the run exits non-zero with a structured error. This makes plumbing failures obvious and is appropriate for a controlled fixture.

Production behavior will be age-tiered rather than universally fail-closed. It must provide a parent override and distinguish a filtering failure from a content verdict so a broken update cannot permanently remove browser access.

The supervisor applies bounded response sizes, inference queue depth, per-image deadlines, a navigation deadline, and a total memory budget. Oversized or decompression-bomb fixtures must produce a controlled error rather than unbounded allocation.

## Nix development environment

The root flake is a practical developer interface, not a new operating-system build system. It pins:

- Rust, Cargo, rustfmt, Clippy, and rust-analyzer;
- native build tooling needed by the Rust dependencies;
- ONNX Runtime and image libraries when dynamically linked;
- Chromium and local browser-test utilities;
- Docker client, QEMU, OVMF, OCR, and ISO helper utilities; and
- formatting, linting, and audit tools.

Expected commands:

```text
nix develop                       enter the complete development shell
nix build                         build the Rust browser-filter binary
nix flake check                   format, lint, unit, and fixture checks
nix run .#infer -- IMAGE          run bounded inference for one local image
nix run .#run                     run the headed local interception demo
nix run .#bench                   benchmark 1, 13, 19, and 62 images
nix run .#check                   run the host formatting, lint, and test suite
```

The binary keeps `doctor` only as an explicitly reserved, unimplemented CLI placeholder, and the host flake does not publish a doctor app. The separate ISO workflow milestone owns the real environment doctor and the future ISO unit, build, QEMU acceptance, and integration apps. That work stays out of the current `nix flake check` because it needs Docker, KVM, network access, mutable caches, and multi-gigabyte artifacts.

The future ISO build app will validate the sibling checkouts and invoke `../omarchy-iso/bin/omarchy-iso-make --keep-pkg-cache --no-boot-offer --local-source ../omarchy ../omarchy-pkgs`. The privileged build continues inside the existing Arch Linux Docker container and `mkarchiso`; Nix will supply the host tools and the stable command surface.

The current ISO harness assumes Arch-host OVMF paths and unconditionally manages host packages through `omarchy-pkg-add`. Our sibling ISO clone receives two small, general adaptations on a local-only branch instead of duplicating it: `OMARCHY_VM_OVMF_CODE`/`OMARCHY_VM_OVMF_VARS_TEMPLATE` overrides with the current Arch paths as defaults, and `OMARCHY_ISO_MANAGE_HOST_DEPS=0` to skip package installation while still validating every required executable and firmware file. Nix points those overrides at `OVMF.fd` in its store.

The ISO builder's local-source mode currently builds only `omarchy-dev`, `omarchy-settings-dev`, and `omarchy-nvim`, and its sync option knows only the main Omarchy tree. The first ISO command therefore proves the baseline toolchain only. After the host pipeline passes, the ISO and package clones gain a generic local-extra-package input and generic guest artifact sync hook on local-only branches; they do not gain a Kids-specific flag.

The flake lock pins developer inputs. Cargo's lock file pins Rust dependencies. Model downloads use a fixed content hash. The headed-run summary reports the Chromium version that actually executed; this identity is not part of the per-image `MetricRecord` schema.

## Test strategy

### Rust unit tests

- preprocessing produces the documented tensor shape, BGR layout, range, right/bottom padding, and alpha/grayscale behavior;
- model-output decoding maps known synthetic tensors to detections;
- page policy blocks the five exact exposed-content classes, allows covered/ambiguous/empty reports, and keeps the deterministic fixture override scoped to its exact loopback marker;
- substituted responses have internally consistent headers;
- metrics serialize without raw content;
- size, time, and queue limits fail predictably.

### Local integration tests

- fixture server returns HTML plus safe, flagged, corrupt, oversized, redirected, cached, and slow images;
- Chromium receives original bytes for allowed fixtures;
- Chromium receives placeholder bytes for the flagged fixture;
- the opaque cover exists before fixture images can paint;
- controller/model failure leaves the cover in place;
- cleanup removes the temporary profile and terminates Chromium;
- 1, 13, 19, and 62-image pages produce complete timing records;
- benchmark runs include three warmups and at least 20 measured iterations, reporting p50/p90/p95; and
- colored-pixel preprocessing tests preserve the implemented tensor layout while Python-reference semantic parity remains an unverified future gate; no checked-in reference golden exists today.

### ISO tests

ISO work begins only after the host pipeline passes. The Rust binary, extension, pinned runtime/model inputs, local-only package metadata, and a clearly labeled controlled-demo launcher are packaged into a local-source ISO. The existing Omarchy ISO harness installs that ISO into a VM. A Kids-owned acceptance scenario runs the local fixture, verifies the installed artifacts, runtime identity, and exact model hash, validates covered and revealed pixels through the fixture runner's in-memory screenshots, preserves the derived JSON evidence, and asserts the deterministic flagged response is absent from the rendered result.

The original `run` command is not a long-lived service and cannot navigate arbitrary sites: it owns a disposable profile, starts its own loopback fixture, and accepts no external URL. The first Kids ISO therefore installed only that controlled demonstrator. The later `browse` supervisor and adult-filter package add persistent top-level HTTP(S) navigation while retaining a private disposable profile, but still do not install a background service, replace `chromium.desktop`, change Omarchy's default-browser routing, or apply a machine-wide Chromium policy. The package remains private and local while the project and model licensing questions are unresolved.

## Performance gates

On the current development machine and with a warm CPU session:

- median single-image inference at or below 25 ms;
- 17-image total inference at or below 350 ms;
- median pause-to-fulfill overhead excluding inference at or below 10 ms per image;
- initial allowed-page reveal no more than 500 ms after the last required fixture response; and
- 62-image workload completes without unbounded memory growth and within 1.5 seconds of added processing time.

These are experiment gates, not promises for all Omarchy hardware. The benchmark records enough detail to establish a minimum supported device later.

## Measured host result

The host experiment was rerun on 2026-09-03 with the exact check, release benchmark, and headed-browser commands recorded in [the experiment results](../docs/experiment-results.md). All three commands completed successfully on an AMD Ryzen 5 7600 host with Chromium 152.0.7977.75, Rust 1.97.1, ONNX Runtime 1.27.1, and the verified model SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.

The benchmark's median ONNX call was 4.528 ms for the 1-image workload. The 62-image total workload was 1.073885 seconds p50 and 1.092141 seconds p95. Its generated color formula repeats every 16 images, so this is a throughput workload rather than 62 distinct image patterns.

The headed fixture intercepted 17 responses, continued 16, deterministically replaced index 5, left zero pauses unresolved, and shut Chromium down cleanly. Three screenshots taken while the flagged response was held for 501 ms contained only the opaque cover color. The revealed screenshot contained all 16 safe fixture colors and the placeholder, while the original flagged color was absent. Reveal occurred 2 ms after the final required response settled, and the disposable profile was removed before the command emitted its successful summary.

This establishes the response-interception, cover, replacement, and cleanup plumbing only. The deterministic flagged route is not a NudeNet accuracy test, and this result does not validate NudeNet accuracy, real-world pornography blocking, adversarial robustness, model suitability or licensing for distribution, video/canvas/CSS-background/`blob:`/service-worker paths, or ISO integration. The current metrics also do not isolate pause-to-fulfill overhead or measure peak memory, so those portions of the original performance gates remain unresolved. The verified model checksum/runtime execution and colored-pixel preprocessing tests do not establish semantic parity with the upstream Python reference; that gate remains explicitly unverified.

## Final installed adult-content-filter package result

The browser is now packaged as the opt-in Omarchy application `omarchy-adult-content-filter`, backed by a private Arch package of the same name. It launches a dedicated disposable managed Chromium without replacing ordinary Chromium, registering a default browser, installing a service, or changing system policy. This is the intended browser-only plugin boundary; child-account and machine-lockdown enforcement are deferred.

The final uniquely tagged ISO is `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-adult-filter-final-20260904-174935-254772802.iso`, 6,209,560,576 bytes, SHA-256 `c3bb3149668e1e054c5942a342e12201cd4e1eacc753664f9566fdaddb2870d2`. It contains one matching `omarchy-adult-content-filter-0.1.0-1-x86_64.pkg.tar.zst` archive, 23,781,775 bytes, SHA-256 `8c6324c3b880af6d87a06cf4abaaaee48c2a0ac70001f4b82fe0515aebb93e2b`. The ISO was built from browser/package source `eb74d00`, package recipe `6ee53e6`, ISO workflow `29a66a2`, and Omarchy `fb39bcd3`; the later acceptance-only compatibility fix is `9d29329`.

The graphical configurator completed all 14 install phases and produced a clean 40 GiB base with SHA-256 `28989d415d91bf1374ddd856c6ba3a7f569bee320a3dd19227233a854510b65c`. Final reuse-base acceptance run `20260904-143640` passed all desktop smoke checks, the normal Omarchy suite in 86 seconds with no failed system or user units, and the installed adult-filter suite. That suite proved the pinned private package inventory, native-Wayland Chromium with developer tools disabled and a disposable profile, one-tab enforcement, clean launcher shutdown, and complete process/profile/VM cleanup.

The installed controlled fixture reported Chromium 152.0.7977.82, ONNX Runtime 1.27.1, the pinned model hash, 17 intercepted images, 16 continuations, one replacement, zero unresolved, and a 2 ms reveal. Three samples proved an opaque cover for an actual 1,501 ms before reveal. Exactly 34 privacy-safe metrics were collected. The managed-launch summary loaded all 76,767 pinned adult-domain entries and rejected an extra page. Host behavior tests cover the earlier domain, SafeSearch, and YouTube request layers plus fail-closed image inference and media blocking connected to a flagged thumbnail.

This result establishes an installable browser plugin and its controlled filtering pipeline. It does not establish model accuracy on real adult content, adversarial resistance, complete coverage of every browser rendering/media path, redistribution clearance, or an OS policy that prevents launching other browsers. Exact evidence and hashes are recorded in [the experiment results](../docs/experiment-results.md).

## Superseded measured installed Kids ISO result

The private installed-ISO milestone passed its final post-review validation on 2026-09-04 without changing the ordinary Omarchy browser default or installing a background service or managed Chromium policy. The final uniquely tagged artifact was built from reviewed Kids `d2f0a2c`, generic ISO workflow `29a66a2`, Omarchy `fb39bcd`, and local package recipe `bb633d6`; the upstream baseline ISO and every earlier Kids artifact remained separate and byte-identical.

The exact approved ISO is `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195.iso`, 6,209,560,576 bytes, SHA-256 `c76830249287bad5235fb9bfb0690a078203ab62cb34cb9387c28e2436dc7f76`. Its sole matching archive is `omarchy-kids-browser-filter-demo-0.1.0-1-x86_64.pkg.tar.zst`, 23,034,995 bytes, SHA-256 `54d8046e070e9f89e2fbb9c720c2128d5d722f15703e038420c0a99ea0d1edd2`. The build completed in 374.367 seconds after clean-environment doctor, ISO unit, Kids check, flake check, and exact-snapshot native Arch preflight all passed.

The real graphical configurator installed that ISO into a fresh 40 GiB qcow2 base in 404.744 seconds. All 14 timing phases report `ok`, and both the installed pacman log and collected configurator log identify the private package. The 6,527,582,208-byte base has SHA-256 `982618b68b74b373ae987a12aec9bb612ed94ad9a02e42a0bdfe229a922f9510`, remained unchanged through overlay acceptance, and passed a non-repairing QEMU 11.1.0 `qemu-img check`. Its copied OVMF vars was mode `0600`. The first bounded console-bootstrap attempt timed out after 120 seconds; the second succeeded and both outcomes are retained.

Canonical reuse-base acceptance with the exact reviewed Omarchy source sync completed in 153.546 seconds. Normal installed Omarchy acceptance passed in 80 seconds with no failed system or user units. The external Kids suite observed the installed package, enforced its complete non-directory allowlist and ownership/modes, observed a new headed Chromium client, ran the dedicated launcher, and collected a nonempty summary, exact 34-line metric stream, and acceptance log. Independent checks reproduced the strengthened summary predicate, exact four-key privacy schema, per-index metric structure, base/overlay integrity, and cleanup.

The approved installed fixture reported ONNX Runtime 1.27.1 and model SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`. It intercepted 17 image responses, continued 16, deterministically replaced index 5, left zero unresolved, revealed after 2 ms, and shut down cleanly. Its 1,500 ms requested and actual hold produced three cover samples and 1,413,000 opaque `[17, 19, 24, 255]` pixels. The reveal sample contained 471,000 pixels, all 16 safe colors, and the magenta placeholder while excluding the original flagged color. All 17 exact 1×1 DOM entries were present, metrics contained 17 inference and 17 policy records with replacement only at index 5, and no disposable profile, Kids-launched Chromium process, QEMU process, or configured listener remained.

The install evidence is under `omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195/runs/20260904-063725`; canonical acceptance evidence is under sibling run `20260904-064741`; complete logs and independent validation records are under `.superpowers/sdd/2026-09-03-kids-iso-controlled-demo-implementation/final-artifact-validation-artifacts/20260904-100917-980113195/`. All 461 files frozen immediately before the build and all 548 initially inventoried prior files passed final SHA-256 verification. The exact identities, hashes, package manifest, timings, retries, and evidence predicates are recorded in [the experiment results](../docs/experiment-results.md).

This approved installed result proves only Arch packaging, offline installation, dedicated controlled-launcher execution, and the harmless 17-image fixture's interception, cover, deterministic replacement, privacy schema, detector identity, and cleanup. It does not show that the current executable can browse arbitrary sites; validate pornography-classifier accuracy or false-positive/recall behavior; survive adversarial or bypass attempts; provide a tamper-resistant cover, supervised service, safe default browser, or machine-wide managed policy; or satisfy model and project redistribution requirements.

### Superseded but successful first installed artifact

The first successful installed artifact remains preserved at `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-kids-demo-20260904-074211-170516200.iso`, 6,209,560,576 bytes, SHA-256 `1fd7379385b64c3746a8f32e94e63226766de19dfb6347f6ec6cbd28da641368`, with matching package SHA-256 `dd8b39035959d92192e3917cf3598e5b288a95415fccbe8fa5f29cab1db61749`. Its install run `20260904-035052` and acceptance run `20260904-040008` passed the then-current workflow in 402.701 and 154.126 seconds. It is successful historical evidence, but it is superseded as the approved result because later review fixes changed the Kids launcher/package bytes and strengthened ISO workflow atomicity, timeouts, cleanup propagation, installed allowlisting, and no-flash assertions. Its complete host evidence remains under the immutable `task-5-artifacts/` directory.

## Security and privacy constraints

- Bind fixture and debugging endpoints to loopback only and use random available ports.
- Use a fresh profile containing no real cookies, credentials, history, or extensions.
- Never disable Chromium's sandbox.
- Never pass `--ignore-certificate-errors`; the fixture uses loopback HTTP.
- Do not persist raw response bodies, decoded images, screenshots containing non-fixture browsing, or model inputs.
- Keep the model process unprivileged and apply input/resource bounds.
- Treat DevTools control as browser-equivalent privilege.
- Do not browse arbitrary internet pages through the experiment command.
- Store only harmless, redistributable fixtures in Git.

## Delivery sequence

1. Establish the Nix flake, Cargo workspace, checks, and harmless fixture corpus.
2. Implement and benchmark the standalone Rust inference path.
3. Launch disposable headed Chromium and prove response pause/release/substitution.
4. Add the document-start cover and screenshot assertions.
5. Add machine-readable benchmark workloads and publish the measured result in this plan.
6. Gather sibling Omarchy ISO and package checkouts and expose delegated build/test commands.
7. Package the proven experiment into a local-source ISO and add a VM acceptance scenario.
8. Reassess architecture, model licensing, browser bypass, and accuracy-corpus work before any production integration.

Each step must remain independently testable. A failed browser experiment must not block inference benchmarking, and a failed ISO build must not prevent the host fixture from running.

## Decision after the experiment

Continue toward a managed Omarchy Kids browser only if the host experiment meets the latency gates, reliably substitutes before reveal, survives the defined failure fixtures, and can be packaged without a Chromium fork or trusted CA. If response interception cannot be made reliable across cache and navigation states, stop before investing in accuracy work and evaluate a deeper Chromium integration separately.
