# Omarchy Kids browser filter

This repository contains a local-only experiment for running a managed Chromium session with ONNX image inference. It is a pipeline spike, not a content-classification product or an accuracy claim.

## Development

Enter the pinned development environment and run the workspace tests:

```bash
nix develop -c cargo test --workspace
```

The binary exposes `doctor`, `infer`, `bench`, and `run`. Those commands are intentionally placeholders until their corresponding experiment stages are implemented.

The development shell and packaged binary provide these environment variables:

- `ORT_DYLIB_PATH`
- `NUDENET_MODEL_PATH`
- `CHROMIUM_BIN`
- `OMARCHY_KIDS_EXTENSION_DIR`
