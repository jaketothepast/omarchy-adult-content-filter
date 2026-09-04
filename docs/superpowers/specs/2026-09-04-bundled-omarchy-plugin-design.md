# Bundled Omarchy Adult Content Filter Plugin Design

## Goal

Publish the managed adult-content-filtering browser as one opt-in Omarchy marketplace plugin. One repository and one enabled plugin must contain and supervise the complete filtering runtime: the managed Chromium controller, local inference engine, ONNX Runtime, NudeNet model, adult-domain policy, document cover, and Omarchy launch UI.

The system Chromium executable remains an external Omarchy runtime dependency. “Bundled browser” means the complete managed browser experience and its supervisor are bundled; it does not mean maintaining a fork of Chromium.

## Scope

The plugin provides a browser-only safety boundary. It owns its Chromium process, disposable browser profile, filtering inputs, and cleanup. It does not create users, remove administrative access, replace the system default browser, prevent another browser from being launched, install a system service, or claim child-account enforcement. Those controls remain a separate future system layer.

The plugin preserves the demonstrated filtering boundary:

1. Block requests matching a pinned adult-domain list before network access.
2. Force Google SafeSearch and YouTube restricted mode.
3. Cover the document until intercepted images have settled.
4. Inspect supported static image responses locally with the pinned NudeNet ONNX model and fail closed on detection or analysis failure.
5. Block only media controls connected to a replaced image.
6. Deny downloads and DevTools, allow one managed page, and remove the disposable profile after exit.

## Chosen architecture

The public repository is an Omarchy plugin and the source of the native runtime. It has a root `manifest.json` declaring both `service` and `bar-widget`, with `keepLoaded: true`.

- `Service.qml` is the plugin lifecycle owner. It launches exactly one supervisor process, reports running/failure state, exposes launch and stop operations to the bar widget, and terminates the process when the plugin is disabled.
- `BarWidget.qml` is the user control. A left click launches or focuses the managed browser; a deliberate stop action terminates it. It does not implement filtering or spawn Chromium directly.
- `bin/omarchy-adult-content-filter` is the narrow runtime boundary. It accepts no arbitrary command or path arguments, refuses root, validates its repository-relative payload, takes a per-user single-instance lock beneath `XDG_RUNTIME_DIR`, creates private runtime state, and executes the native supervisor.
- `runtime/` contains the reviewed x86-64 supervisor, ONNX Runtime, model, domain policy, extension, third-party licenses, and a checksum manifest. The supervisor launches the system `/usr/bin/chromium` with the existing managed flags and owns Chromium, CDP interception, local inference, deadlines, fail-closed policy, and cleanup.
- The Rust workspace, Nix development environment, reproducible Arch build recipe, tests, and evidence documentation remain in the same repository. There is no second source or package repository in the user-facing installation path.

The plugin ID is permanent and namespaced: `io.github.jaketothepast.adult-content-filter`. The marketplace listing uses category `System` and tags `security`, `launcher`, and `ai`.

## Alternatives rejected

### Thin plugin over a separately installed Arch package

This keeps Git small, but marketplace installation would clone only a UI and leave the actual browser absent. It creates two version and removal lifecycles and violates the requested single-unit boundary.

### First-run download or build

This avoids committing native assets, but turns enablement into mutable remote code or binary acquisition. It also adds network/build failure paths, can trigger marketplace remote-build findings, and prevents exact plugin-commit review from covering the running bytes.

### Bundled Chromium fork

This would make the clone extremely large and transfer browser patch/update responsibility to this project. The managed controller already confines the system Chromium instance, so bundling Chromium itself adds risk without strengthening the demonstrated browser-only boundary.

## Runtime lifecycle

1. Omarchy clones the public repository and leaves the plugin disabled for review.
2. The user enables the plugin, loading its kept service and bar widget.
3. The widget asks the service to launch the repository-local wrapper.
4. The wrapper validates the per-user runtime directory, payload file types, expected SHA-256 values, stable ONNX soname chain, and system Chromium path. Any failure stops before Chromium starts.
5. The wrapper obtains a non-blocking per-user lock. A second launch focuses the existing browser when possible or returns a clear already-running result; it never starts an unsupervised second instance.
6. The native supervisor creates a disposable profile, starts Chromium, attaches CDP interception before navigation, and applies all filtering layers.
7. Closing Chromium makes the supervisor verify that response pauses, worker tasks, browser processes, and profile cleanup have settled before it reports success.
8. Disabling or removing the plugin asks the service-owned process to stop. The supervisor performs the same bounded cleanup. Runtime-only state is beneath `XDG_RUNTIME_DIR`; removing the plugin checkout removes the persistent unit.

## Failure behavior

Safety-relevant errors fail closed:

- Missing, corrupt, incorrectly typed, or incorrectly linked runtime assets prevent launch.
- Domain policy load/count failure prevents navigation.
- Image decode, inference, timeout, or response-settlement failure replaces the image.
- A replaced image taints only associated media.
- Losing CDP interception, the inference worker, or the browser supervision boundary ends the managed session.
- A supervisor that cannot stop Chromium cleanly returns failure; the plugin never reports a clean stop while the owned process is still alive.

The QML layer is not trusted with policy decisions. A QML crash or hot reload cannot create an unfiltered Chromium instance because only the validated wrapper may start the supervisor.

## Distribution and licensing

Original project source and plugin QML/scripts will be licensed under AGPL-3.0. The bundle will preserve component-specific licenses and notices:

- NudeNet model and repository license evidence: AGPL-3.0, pinned to the reviewed upstream commit.
- ONNX Runtime: MIT plus upstream third-party notices.
- StevenBlack adult-domain list: upstream MIT license and exact source identity.

The root README will identify every external dependency and the exact architecture limitation (`x86_64` Omarchy). It will state that the repository contains a native executable, that marketplace validation is not a security audit, and that the browser-only plugin is modifiable by the owning user and is not an anti-tamper child account.

## Marketplace hardening alignment

- One public GitHub repository, one root manifest, one permanent namespaced ID.
- Repository-root README, license, and optional preview.
- No install/uninstall hooks, `sudo`, `pkexec`, sudoers policy, service unit, remote build, curl-to-shell, or mutable remote execution.
- Entry points remain relative and inside the plugin root.
- Runtime state uses the owner-only `XDG_RUNTIME_DIR`, never a predictable shared `/tmp` PID file.
- The repository intentionally contains native ELF files. The expected automated-baseline disposition is `review-required` with `bundled-executable-binary`; the submission will disclose that capability for maintainer review rather than trying to evade detection.
- Publication and any later update are bound to the exact reviewed commit.

## Verification

Automated checks must prove:

- Exact manifest schema, ID, kinds, entry points, and kept-service declaration.
- QML service owns the process and the widget delegates launch/stop without constructing its own Chromium command.
- Wrapper argument rejection, root rejection, safe runtime directory, single-instance lock, relative-path confinement, file-type validation, checksum validation, and exact environment/command construction.
- Bundle allowlist, executable/library modes, symlink targets, ELF dependency closure, ONNX Runtime API identity, model/domain hashes, and absence of unlisted persistent files.
- Marketplace scanner limits and deterministic finding patterns against the final tracked tree.
- Existing Rust unit/integration tests, controlled headed filtering proof, privacy-safe metric schema, and cleanup.

Release verification must build a fresh runtime from the exact source commit, compare it to the bundled manifest, install the plugin into a disposable Omarchy QEMU guest, enable it through the real plugin path, launch through the bar widget/service boundary, and rerun normal plus adult-filter acceptance. The final report must preserve exact repository commit, binary/model/runtime/policy hashes, browser summary, process/profile cleanup, and marketplace validation result.

## Publication flow

Create `jaketothepast/omarchy-adult-content-filter` as a public repository and push only the reviewed plugin branch as `main`. Do not push or alter the Omarchy, ISO, or package sibling repositories.

Before opening the marketplace issue, show the owner the exact required issue title and body. Create the issue only after the owner confirms the repository/license/ownership/configuration checklist, as required by the marketplace submission guide.
