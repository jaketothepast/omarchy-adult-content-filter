# Omarchy Adult Content Filter

This repository builds an opt-in, browser-only adult-content filtering package for Omarchy. The installed application launches a dedicated managed Chromium instance with its own disposable profile; it does not modify the user's ordinary Chromium profile or make itself the system default browser.

Filtering is layered:

1. A pinned adult-domain list blocks matching requests before network access.
2. Google searches are rewritten to force SafeSearch, and YouTube requests receive the strict restricted-mode header.
3. A document-start cover prevents unreviewed page imagery from flashing onscreen.
4. Successful static JPEG and PNG responses are inspected locally with the pinned NudeNet ONNX model. Explicit detections are replaced; decoding, model, timeout, and policy failures are also replaced.
5. Replacing an image taints its connected media control, so a thumbnail associated with a video prevents that video from playing without blocking unrelated videos on the page.

The managed instance permits one page, denies downloads, disables DevTools, accepts only complete HTTP(S) top-level navigations, and fails closed when it cannot settle an intercepted response or cleanly maintain its supervision boundary.

## Installable Omarchy package

The plugin-facing package and command are both named `omarchy-adult-content-filter`. The package contains the Rust supervisor, private ONNX Runtime, pinned NudeNet model, pinned domain policy, cover extension, desktop entry, and notices. It deliberately installs no system service, autostart entry, MIME association, default-browser handler, user account, sudo rule, or global Chromium policy.

After installation, launch **Omarchy Adult Content Filter** from the app menu or run:

```bash
omarchy-adult-content-filter
```

The public launcher accepts no arguments and refuses to run as root. Runtime state and the Chromium profile stay beneath its private directory in `XDG_RUNTIME_DIR` and are removed when the browser exits.

This browser-only package does not prevent a user from launching another browser, replacing the executable, or changing the machine when that user already has administrative access. Account restrictions and OS-level enforcement are intentionally deferred to a separate Omarchy Kids system layer; they are not part of this plugin.

## Host development

Run the full checks, local inference benchmark, controlled headed proof, or persistent browser from the repository root:

```bash
nix flake check
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 1500 --assert-no-flash --json
nix run .#browse
```

The controlled fixture contains only generated color images. It deterministically exercises one replacement without storing or downloading explicit imagery. JSON summaries contain runtime/model identity and aggregate counters; per-stage JSONL contains only `stage`, `verdict`, `fixture_index`, and `elapsed_micros`.

## Build and validate in Omarchy

The product-named wrappers use the reviewed local Omarchy, ISO, package, and filter checkouts:

```bash
nix run .#adult-filter-iso-build
nix run .#adult-filter-iso-test -- /absolute/path/to/omarchy-adult-filter.iso --reuse-base --no-preview
```

The installed acceptance proof checks exact package contents, ownership, permissions, model and policy identities, managed-browser launch flags, the headed controlled fixture, privacy-safe telemetry, profile cleanup, and process cleanup.

## Scope limits

This is a filtering prototype, not a guarantee that every adult page or adversarial rendering will be detected. The model path currently covers ordinary static JPEG/PNG image responses; canvas, WebGL, CSS background images, encrypted media streams, audio, extensions outside the packaged surface, and hostile browser/OS modification remain outside the demonstrated boundary. The pinned model and domain list also require a separate redistribution review before public release.

Measured benchmark, host-browser, and controlled ISO results are recorded in [docs/experiment-results.md](docs/experiment-results.md). Internal Rust and environment-variable names retain `omarchy-kids` compatibility until a later code-only migration; the installed product surface is `omarchy-adult-content-filter`.
