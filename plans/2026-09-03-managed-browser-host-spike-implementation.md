# Managed-Browser Host Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reproducible Rust experiment that runs a real ONNX model on image responses paused inside a disposable headed Chromium session, substitutes a deterministic fixture, and reports end-to-end latency.

**Architecture:** A single Rust binary owns an Axum fixture server, one warm ONNX Runtime session, a Chromiumoxide controller, policy decisions, and JSON metrics. A fixture-only Manifest V3 extension hides the page at document start; a root Nix flake pins the compiler, ONNX Runtime, model, Chromium, and test tools.

**Tech Stack:** Rust 1.97.1, Tokio, Axum, Chromiumoxide 0.9.1, `ort` 2.0.0-rc.13, ONNX Runtime 1.27.1, `image` 0.25.10, Manifest V3, Nix flakes.

**Spec:** `plans/managed-browser-content-filter.md`

## Global Constraints

- Run only against a loopback fixture origin and a unique disposable Chromium profile.
- Keep Chromium's sandbox enabled and never pass `--ignore-certificate-errors`.
- Use NudeNet 320n only as a spike dependency, pinned to SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.
- Dynamically load Nixpkgs ONNX Runtime through `ORT_DYLIB_PATH`; do not enable `ort`'s binary downloader.
- Log timings and fixture metadata only; never log or persist response bodies or decoded tensors.
- Bound encoded bytes, decoded dimensions, queue depth, and operation deadlines.
- Model accuracy, internet browsing, video/canvas/blob handling, production browser policy, and tamper resistance are out of scope.

---

### Task 1: Reproducible Rust workspace

**Files:**
- Modify: `.gitignore`
- Create: `Cargo.toml`
- Create: `browser-extension/.gitkeep`
- Create: `crates/browser-filter/Cargo.toml`
- Create: `crates/browser-filter/src/lib.rs`
- Create: `crates/browser-filter/src/main.rs`
- Create: `flake.nix`
- Create: `flake.lock`
- Create: `README.md`

**Interfaces:**
- Produces: binary `omarchy-kids-browser-filter`; library crate `omarchy_kids_browser_filter`.
- Produces: CLI subcommands `doctor`, `infer`, `bench`, and `run`.
- Produces: environment contract `ORT_DYLIB_PATH`, `NUDENET_MODEL_PATH`, `CHROMIUM_BIN`, and `OMARCHY_KIDS_EXTENSION_DIR`.

- [ ] **Step 1: Write the failing CLI smoke test**

Create a `#[test]` in `src/main.rs` using `clap::CommandFactory`:

```rust
#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}
```

- [ ] **Step 2: Verify the empty workspace cannot run it**

Run: `cargo test --workspace`

Expected: FAIL because no Cargo workspace or `Cli` exists.

- [ ] **Step 3: Add the workspace and CLI skeleton**

Pin dependencies in the workspace, including:

```toml
[workspace]
members = ["crates/browser-filter"]
resolver = "2"

[workspace.dependencies]
anyhow = "1"
axum = "0.8"
base64 = "0.22"
chromiumoxide = "0.9.1"
clap = { version = "4.5", features = ["derive"] }
futures = "0.3"
image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "gif", "webp"] }
ort = { version = "=2.0.0-rc.13", default-features = false, features = ["std", "load-dynamic", "api-27"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "signal", "time"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Define `Cli` with the four subcommands. Each unimplemented command returns an explicit `anyhow::bail!("<name> is not implemented")`; `--help` succeeds. Create `browser-extension/.gitkeep` so the flake path exists before Task 6 adds the real extension, then generate `Cargo.lock` with `cargo generate-lockfile` after creating the manifests.

- [ ] **Step 4: Add the Nix shell and package**

`flake.nix` must expose `devShells.x86_64-linux.default`, `packages.x86_64-linux.default`, `checks.x86_64-linux.default`, `formatter.x86_64-linux`, and `apps.x86_64-linux.{doctor,infer,bench,run,check}`. Fetch the model from:

```text
https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/nudenet/320n.onnx
```

with:

```nix
hash = "sha256-wV2Cc62tLQqS8BTMaastbDEaBnd6VVRfLE60b1GRHw8=";
```

Set:

```nix
ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/libonnxruntime.so.${pkgs.onnxruntime.version}";
NUDENET_MODEL_PATH = model;
CHROMIUM_BIN = "${pkgs.chromium}/bin/chromium";
OMARCHY_KIDS_EXTENSION_DIR = ./browser-extension;
```

Use `rustPlatform.buildRustPackage` with `cargoLock.lockFile = ./Cargo.lock`; its `checkPhase` runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.

- [ ] **Step 5: Verify the toolchain**

Run:

```bash
cargo test --workspace
cargo generate-lockfile
nix flake lock
nix flake show
nix develop -c cargo test --workspace
nix build
```

Expected: every command exits 0; `result/bin/omarchy-kids-browser-filter --help` lists all four commands.

- [ ] **Step 6: Commit**

```bash
git add .gitignore Cargo.toml Cargo.lock browser-extension crates flake.nix flake.lock README.md
git commit -m "Establish reproducible browser filter workspace"
```

### Task 2: Bounded NudeNet preprocessing and output decoding

**Files:**
- Create: `crates/browser-filter/src/inference.rs`
- Create: `crates/browser-filter/tests/infer_cli.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Modify: `crates/browser-filter/src/main.rs`

**Interfaces:**
- Produces: `ModelConfig { model_path: PathBuf, runtime_path: PathBuf, max_encoded_bytes: usize, max_pixels: u64 }`.
- Produces: `Detector::load(ModelConfig) -> Result<Detector>`.
- Produces: `Detector::detect(&mut self, encoded: &[u8]) -> Result<InferenceReport>`.
- Produces: `preprocess(encoded, limits) -> Result<PreparedImage>` and `decode_detections(output, original_size) -> Result<Vec<Detection>>`.

- [ ] **Step 1: Write preprocessing tests**

Generate 2×1 RGB and RGBA PNG bytes in test code. Assert `preprocess` returns shape `[1, 3, 320, 320]`, top-left anchoring, black bottom padding, values in `0.0..=1.0`, intentional BGR channel order, and the original dimensions. Add rejection tests for encoded bodies larger than 16 MiB and decoded images larger than 40 megapixels.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter inference::tests`

Expected: FAIL because `inference` does not exist.

- [ ] **Step 3: Implement preprocessing**

Use `image::ImageReader` with explicit limits, convert to RGB8, allocate one `max(width, height)` black square, copy source pixels at `(0, 0)`, resize with triangle filtering, and fill one contiguous NCHW `Vec<f32>` in B, G, R plane order.

- [ ] **Step 4: Write output-decoding tests**

Construct a `[1, 22, 2100]` zero tensor with two overlapping candidates. Assert score filtering at 0.20, effective final threshold 0.25, center-to-corner conversion, square-to-original scaling, clipping, label order, and class-agnostic NMS at IoU 0.45.

- [ ] **Step 5: Implement output decoding and `Detector`**

Initialize ONNX Runtime exactly once with `ort::init_from(runtime_path).commit()`. Build a graph-optimized session, feed input `images`, read output `output0`, and return:

```rust
pub struct InferenceReport {
    pub detections: Vec<Detection>,
    pub encoded_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub decode_micros: u64,
    pub preprocess_micros: u64,
    pub inference_micros: u64,
    pub postprocess_micros: u64,
}
```

- [ ] **Step 6: Connect the `infer` command**

`infer PATH` reads one local image subject to the encoded-size limit and prints exactly one JSON object containing dimensions, timings, the pinned model hash, and detections. It never prints the path or image bytes. The CLI integration test generates a harmless PNG in a temporary directory, runs the real binary against the pinned model, and parses its single JSON line without committing a binary fixture.

- [ ] **Step 7: Verify and commit**

Run:

```bash
nix develop -c cargo fmt --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace
```

Expected: checks pass and inference JSON names the pinned model hash.

```bash
git add crates/browser-filter
git commit -m "Add bounded local ONNX image inference"
```

### Task 3: Harmless fixtures, deterministic policy, and metrics

**Files:**
- Create: `crates/browser-filter/src/fixture.rs`
- Create: `crates/browser-filter/src/policy.rs`
- Create: `crates/browser-filter/src/metrics.rs`
- Modify: `crates/browser-filter/src/lib.rs`

**Interfaces:**
- Produces: `FixtureServer::start(image_count: usize, flagged_index: usize) -> Result<FixtureServer>` and `FixtureServer::url(&self) -> Url`.
- Produces: routes `/`, `/image/:index.png`, `/redirect.png`, `/corrupt.png`, `/slow/:millis.png`, and `/health`.
- Produces: `Policy::decide(&self, request_url: &Url, report: &InferenceReport) -> Verdict` where `Verdict` is `Allow` or `Replace { reason: "deterministic-fixture" }`.
- Produces: `MetricRecord` serialized as one JSON object per line.

- [ ] **Step 1: Write failing fixture and policy tests**

Assert the server binds to `127.0.0.1:0`, `/` emits the requested number of image elements, PNGs have distinct deterministic colors, the flagged URL produces `Replace` only after receiving an `InferenceReport`, ordinary URLs produce `Allow`, and serialized metrics contain no URL/path/body/tensor fields.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter --lib`

Expected: FAIL because the three modules do not exist.

- [ ] **Step 3: Implement the fixture server and policy**

Generate all PNGs in memory with the `image` crate. Mark the chosen response with `X-Omarchy-Kids-Fixture: flagged` and keep the same marker in the URL so the Fetch event can identify it without logging. Require a completed inference report before applying the override.

- [ ] **Step 4: Implement safe metrics**

Use enums for stage and verdict. Provide `MetricSink::write(&MetricRecord)` that writes one line through a generic `Write`; test against a byte vector and deserialize it again.

- [ ] **Step 5: Verify and commit**

Run: `nix run .#check`

Expected: formatting, Clippy, and all workspace tests pass.

```bash
git add crates/browser-filter
git commit -m "Add controlled browser-filter fixtures and policy"
```

### Task 4: Standalone inference benchmark

**Files:**
- Create: `crates/browser-filter/src/benchmark.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Modify: `crates/browser-filter/src/main.rs`

**Interfaces:**
- Produces: `run_benchmark(detector, BenchmarkConfig) -> Result<BenchmarkSummary>`.
- Consumes: `Detector::detect` and generated harmless 1280×720 JPEG bytes.
- CLI: `bench --iterations 20 --warmups 3 --json`.

- [ ] **Step 1: Write failing percentile and workload tests**

Assert nearest-rank p50/p90/p95 calculations for fixed values and assert default workloads are exactly `[1, 13, 19, 62]` with three warmups and 20 measured iterations.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter benchmark::tests`

Expected: FAIL because the benchmark module does not exist.

- [ ] **Step 3: Implement the benchmark**

Generate deterministic 1280×720 JPEGs at quality 80. Reuse one warm detector, report per-stage p50/p90/p95 and total workload time, and include CPU model, ONNX Runtime version, model hash, image count, encoded-byte median, and release/debug status. Never emit image data.

- [ ] **Step 4: Verify and commit**

Run:

```bash
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#check
```

Expected: four valid JSON summaries and all checks pass.

```bash
git add crates/browser-filter
git commit -m "Benchmark local image inference workloads"
```

### Task 5: Chromium response interception

**Files:**
- Create: `crates/browser-filter/src/browser.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Modify: `crates/browser-filter/src/main.rs`

**Interfaces:**
- Produces: `BrowserExperiment::run(ExperimentConfig) -> Result<ExperimentSummary>`.
- Consumes: `FixtureServer`, one detector worker, `Policy`, and `MetricSink`.
- Produces: safe-response `continueResponse`, flagged-response `fulfillRequest`, and a count of unresolved pauses that must end at zero.

- [ ] **Step 1: Write failing configuration and header tests**

Assert `ExperimentConfig` rejects non-loopback URLs, a reused/default Chromium profile, a missing extension, image count outside `1..=100`, and timeouts over 30 seconds. Assert replacement headers contain only `Content-Type: image/png` and `Cache-Control: no-store`.

- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter browser::tests`

Expected: FAIL because the browser module does not exist.

- [ ] **Step 3: Launch Chromium and configure Fetch**

Use `BrowserConfig::builder().chrome_executable(CHROMIUM_BIN).with_head().user_data_dir(tempdir).extension(OMARCHY_KIDS_EXTENSION_DIR).window_size(1280, 800)`. Start the Chromiumoxide handler task, create `about:blank`, register `EventRequestPaused`, and enable one Fetch response-stage pattern for `ResourceType::Image`. Do not call Chromiumoxide's catch-all `enable_request_intercept()` helper.

- [ ] **Step 4: Resolve every pause exactly once**

For HTTP 200 image responses, read and decode `GetResponseBodyParams`; execute bounded inference in `spawn_blocking`; call argument-free `ContinueResponseParams` for allowed content or `FulfillRequestParams` with base64 placeholder PNG for replacement. Continue redirects, 204/304, and response-error events without body reads. Wrap acquisition and inference in timeouts and account for every request id in an unresolved set.

- [ ] **Step 5: Connect the `run` command**

`run --images 17 --flagged-index 5 --json` starts the fixture, runs Chromium, waits for all fixture images, evaluates DOM pixel metadata, emits the summary, calls `browser.close()` and `browser.wait()`, then lets the temporary directory delete the profile.

- [ ] **Step 6: Verify the headed smoke test and commit**

Run:

```bash
nix run .#run -- --images 17 --flagged-index 5 --json
nix run .#check
```

Expected: Chromium visibly opens only the local fixture; summary reports 17 intercepted, 16 continued, one replaced, zero unresolved, and a clean shutdown.

```bash
git add crates/browser-filter
git commit -m "Intercept and classify Chromium image responses"
```

### Task 6: Document-start cover and no-flash fixture assertions

**Files:**
- Create: `browser-extension/manifest.json`
- Create: `browser-extension/cover.css`
- Modify: `crates/browser-filter/src/browser.rs`
- Modify: `crates/browser-filter/src/fixture.rs`

**Interfaces:**
- Produces: `html:not([data-omarchy-kids-ready])` cover contract.
- Produces: experiment screenshot samples while the delayed flagged response is paused and after reveal.

- [ ] **Step 1: Write failing extension-contract tests**

Parse `manifest.json` and assert Manifest V3, `<all_urls>`, `run_at=document_start`, `all_frames=true`, `match_about_blank=true`, and declarative `cover.css`. Assert CSS hides document children until `data-omarchy-kids-ready` exists.

- [ ] **Step 2: Confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter extension_contract`

Expected: FAIL because extension files do not exist.

- [ ] **Step 3: Implement the cover**

Use only declarative content-script CSS. It sets an opaque `#111318` root background and `visibility: hidden !important` on root children until the readiness attribute appears. Do not request extension permissions.

- [ ] **Step 4: Add screenshot assertions**

Hold the flagged fixture response for 500 ms. Capture at least three screenshots during the hold and assert their sampled content region contains only the cover color. After all responses resolve, set the readiness attribute, capture again, and assert the safe fixture colors and placeholder color are present while the flagged fixture color is absent.

- [ ] **Step 5: Verify and commit**

Run:

```bash
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
nix run .#check
```

Expected: the no-flash assertion passes and temporary screenshots are deleted at exit.

```bash
git add browser-extension crates/browser-filter
git commit -m "Cover pages until intercepted images are safe"
```

### Task 7: Publish experiment results and limitations

**Files:**
- Modify: `README.md`
- Modify: `plans/managed-browser-content-filter.md`
- Create: `docs/experiment-results.md`

**Interfaces:**
- Consumes: release-mode benchmark and headed-browser JSON.
- Produces: reproducible commands, hardware/runtime metadata, measured p50/p90/p95, pass/fail against gates, and unresolved production gaps.

- [ ] **Step 1: Run final evidence commands**

Run:

```bash
nix flake check
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
```

Expected: all commands pass before results are documented.

- [ ] **Step 2: Write results without overclaiming**

Record exact machine, Chromium, Rust, ONNX Runtime, model hash, workload sizes, timing percentiles, intercepted/replaced counts, and screenshots assertion result. State explicitly that deterministic replacement proves plumbing only and NudeNet accuracy/licensing remain unresolved.

- [ ] **Step 3: Verify docs and commit**

Run: `rg -n 'TBD|TODO|FIXME' README.md docs plans`

Expected: no unresolved placeholders.

```bash
git add README.md docs/experiment-results.md plans/managed-browser-content-filter.md
git commit -m "Document managed-browser experiment results"
```
