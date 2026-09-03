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

The measured 2026-09-03 environment, complete p50/p90/p95 benchmark output, browser assertions, performance-gate assessment, and limitations are in [docs/experiment-results.md](docs/experiment-results.md).

## Development

Enter the pinned development environment and run the workspace tests:

```bash
nix develop -c cargo test --workspace
```

The binary exposes four experiment commands:

- `doctor` validates required runtime inputs.
- `infer` runs standalone model inference.
- `bench` measures the 1-, 13-, 19-, and 62-image workloads.
- `run` launches the controlled headed-browser experiment.

The development shell and packaged binary provide these environment variables:

- `ORT_DYLIB_PATH`
- `NUDENET_MODEL_PATH`
- `CHROMIUM_BIN`
- `OMARCHY_KIDS_EXTENSION_DIR`

## Scope limits

The experiment does not validate NudeNet accuracy, model suitability, adversarial robustness, or model licensing and training-data provenance for distribution. It also does not cover video, canvas or WebGL rendering, CSS background images, `data:` or `blob:` URLs, service-worker and cache variants, dynamically loaded content, hostile-page cover bypasses, browsers outside the supervised Chromium process, or ISO integration.
