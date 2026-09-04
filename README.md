# Omarchy Kids browser filter

This repository contains a local-only host experiment for running a managed Chromium session with ONNX image inference. The controlled fixture demonstrates response-stage image interception, an opaque pre-reveal cover, deterministic replacement, clean browser shutdown, and disposable-profile removal. It is a pipeline proof, not a content-classification product, a NudeNet accuracy result, or evidence of real-world pornography blocking.

## Reproduce the host experiment

Run the full checks, the release benchmark, and the headed-browser proof from the repository root:

```bash
nix flake check
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
```

The browser command starts stock headed Chromium with a disposable profile and a loopback-only fixture. It runs the model on all 17 harmless fixtures, allows 16 responses, and replaces the fixture whose URL carries the deterministic test marker. That marker deliberately overrides the model verdict so the replacement path is exercised without storing explicit imagery.

With `--json`, `run` writes one headed-experiment summary to stdout, including controlled-fixture DOM RGBA metadata. Its per-stage, per-image `MetricRecord` JSONL is written separately to stderr and contains exactly `stage`, `verdict`, `fixture_index`, and `elapsed_micros`. `bench --json` writes four workload summaries to stdout as JSONL, including model/runtime identity, workload configuration, encoded-byte median, and p50/p90/p95 timing objects. These outputs contain no raw image bytes or URLs. Rich run IDs, cache outcomes, pause/cover breakdowns, and memory measurements are future telemetry targets, not current fields.

The measured 2026-09-03 environment, complete p50/p90/p95 benchmark output, browser assertions, performance-gate assessment, and limitations are in [docs/experiment-results.md](docs/experiment-results.md).

## Drive the host-side managed browser

```bash
nix run .#browse
nix run .#browse -- --url https://example.com --json
```

`browse` opens one headed Chromium tab with a new disposable profile and remains attached until the tab/browser is closed or the terminal receives Ctrl-C. A successful HTTP 200 static JPEG or non-animated PNG continues only after local inference returns `Policy::Allow`. Redirects without a rendered body continue. Animated and other formats, plus every body-read, size, decode, model, worker, or policy-processing failure, replace that individual image with the placeholder. Body acquisition, inference, and CDP response settlement each have a five-second deadline; browser launch and cleanup have a thirty-second deadline. There is no total browsing-session deadline. Additional page targets are closed, and failure to close or settle a response closes the managed browser.

The document-start cover stays opaque until the active top-level loader reports `load` and no intercepted image response remains unresolved. The final summary contains only model/runtime identity and aggregate counts—never URLs, titles, response bytes, paths, or history. Closing the browser removes the disposable profile.

This is a supervised engineering prototype, not a child-safe browser. It does not yet inspect video, audio, canvas, WebGL, CSS backgrounds, `data:` or `blob:` content, downloads, browser chrome, DevTools, or adversarial/tampered pages. Subframes remain covered because this version reveals only the supervised top-level document. Do not use it as unsupervised child protection.

## Build and test the private controlled demo ISO

```bash
nix run .#kids-iso-build
nix run .#kids-iso-test -- /absolute/path/to/omarchy-kids-demo.iso --reuse-base --no-preview
```

This is a private, non-redistributable controlled demo for the 17-image local fixture only. It does not establish arbitrary-site filtering, pornography-classifier accuracy, default-browser enforcement, tamper resistance, supervision, or redistribution rights.

## Development

Enter the pinned development environment and run the workspace tests:

```bash
nix develop -c cargo test --workspace
```

The flake exposes five runnable experiment apps:

- `infer` runs standalone model inference.
- `bench` measures the 1-, 13-, 19-, and 62-image workloads.
- `run` launches the controlled headed-browser experiment.
- `browse` launches the persistent single-tab host prototype.
- `check` runs formatting, lint, and workspace tests.

The binary reserves a `doctor` subcommand as an explicitly unimplemented placeholder. It exits with `doctor is not implemented` and is not exposed as a Nix app; the separate ISO workflow milestone owns the real environment doctor.

The development shell and packaged binary provide these environment variables:

- `ORT_DYLIB_PATH`
- `NUDENET_MODEL_PATH`
- `CHROMIUM_BIN`
- `OMARCHY_KIDS_EXTENSION_DIR`

## Scope limits

The experiment does not validate NudeNet accuracy, model suitability, adversarial robustness, or model licensing and training-data provenance for distribution. A checked-in Python-reference golden was not produced, so semantic parity with the upstream reference implementation remains unverified; the model hash/runtime checks and colored-pixel preprocessing tests do not establish that parity. The experiment also does not cover video, canvas or WebGL rendering, CSS background images, `data:` or `blob:` URLs, service-worker and cache variants, dynamically loaded content, hostile-page cover bypasses, browsers outside the supervised Chromium process, or ISO integration.
