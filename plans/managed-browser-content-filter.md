# Plan: Managed-browser content filtering for Omarchy Kids

## Problem

Hostname filters are useful but cannot inspect the URL path or content of an HTTPS page. A separate pre-rendering proxy would need to reproduce the child's authenticated browser session, execute untrusted pages twice, and delay every navigation by roughly another page load. Omarchy Kids needs a local experiment that answers a narrower question first: can stock Chromium pause the exact image responses it will display, classify them locally in Rust, and substitute a safe response before the original pixels become visible?

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

- the model's detections and confidence values; and
- a deterministic marker belonging only to the controlled flagged fixture.

All images execute the same decode and inference path. The deterministic marker overrides the final fixture verdict so the experiment always exercises both release and replacement behavior. Production page aggregation, age tiers, contextual classification, and parent overrides remain later milestones.

### Page cover

A Manifest V3 extension loaded only into the disposable profile injects declarative CSS at `document_start`, hiding document children until the root has a readiness attribute. After every initial fixture image has been resolved, the supervisor sets that attribute through CDP for an allowed page or leaves the cover in place and shows a local block state. The page never receives model details or privileged control messages.

The readiness attribute is deliberately only a pipeline-spike mechanism: hostile page JavaScript could set it, and extension CSS cannot cover Chromium-internal pages. A production cover must be extension-owned and tamper-resistant. The experiment records screenshots repeatedly while one fixture is paused and after the reveal point. These demonstrate the controlled sequence but do not prove that every Chromium cache and navigation path is flash-free; those paths have explicit later acceptance gates.

### Metrics

Every run writes newline-delimited JSON to standard output. Records include a run identifier, Chromium version, model checksum, image count and dimensions, response bytes, cache outcome, decode time, preprocessing time, inference time, policy time, pause-to-fulfill time, initial-cover duration, and final verdict. Raw image bytes, URLs beyond the loopback fixture origin, cookies, and page bodies are never logged.

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
nix run .#run                     run the headed local interception demo
nix run .#bench                   benchmark 1, 13, 19, and 62 images
nix run .#doctor                  validate browser, model runtime, Docker, KVM, firmware, and sibling repositories
nix run .#iso-unit                run the ISO repository's VM-free tests
nix run .#iso-build               call Omarchy ISO's local-source Docker build
nix run .#iso-test                call its QEMU acceptance harness
nix run .#iso-integration         call its QEMU integration harness
```

`iso-build` validates the sibling checkouts and invokes `../omarchy-iso/bin/omarchy-iso-make --keep-pkg-cache --no-boot-offer --local-source ../omarchy ../omarchy-pkgs`. The privileged build continues inside the existing Arch Linux Docker container and `mkarchiso`; Nix supplies the host tools and the stable command surface. ISO and QEMU work stays out of `nix flake check` because it needs Docker, KVM, network access, mutable caches, and multi-gigabyte artifacts.

The current ISO harness assumes Arch-host OVMF paths and unconditionally manages host packages through `omarchy-pkg-add`. Our sibling ISO fork receives two small, general adaptations instead of duplicating it: `OMARCHY_VM_OVMF_CODE`/`OMARCHY_VM_OVMF_VARS_TEMPLATE` overrides with the current Arch paths as defaults, and `OMARCHY_ISO_MANAGE_HOST_DEPS=0` to skip package installation while still validating every required executable and firmware file. Nix points those overrides at `OVMF.fd` in its store.

The ISO builder's local-source mode currently builds only `omarchy-dev`, `omarchy-settings-dev`, and `omarchy-nvim`, and its sync option knows only the main Omarchy tree. The first ISO command therefore proves the baseline toolchain only. After the host pipeline passes, the ISO and package forks gain a generic local-extra-package input and generic guest artifact sync hook; they do not gain a Kids-specific flag.

The flake lock pins developer inputs. Cargo's lock file pins Rust dependencies. Model downloads use a fixed content hash. The system Chromium used for the first experiment is reported in metrics rather than pinned because compatibility with Omarchy's real browser package is what the experiment needs to test.

## Test strategy

### Rust unit tests

- preprocessing produces the documented tensor shape, BGR layout, range, right/bottom padding, and alpha/grayscale behavior;
- model-output decoding maps known synthetic tensors to detections;
- page policy treats the deterministic fixture marker as blocked and ordinary fixtures as allowed;
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
- Rust inference matches a checked-in metadata-only golden result produced by the reference Python implementation before speed is compared.

### ISO tests

ISO work begins only after the host pipeline passes. The Rust binary, extension, model-fetch/package metadata, service, and policies are packaged into a local-source ISO. The existing Omarchy ISO harness installs that ISO into a VM. A new acceptance scenario runs the local fixture, verifies service health and managed policy, captures the covered and revealed states, and asserts the deterministic flagged response is absent from the rendered result.

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

This establishes the response-interception, cover, replacement, and cleanup plumbing only. The deterministic flagged route is not a NudeNet accuracy test, and this result does not validate NudeNet accuracy, real-world pornography blocking, adversarial robustness, model suitability or licensing for distribution, video/canvas/CSS-background/`blob:`/service-worker paths, or ISO integration. The current metrics also do not isolate pause-to-fulfill overhead or measure peak memory, so those portions of the original performance gates remain unresolved.

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
