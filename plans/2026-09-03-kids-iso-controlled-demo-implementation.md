# Omarchy Kids ISO Controlled Demo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a private, local Omarchy ISO that installs and runs the existing managed-Chromium filtering experiment, then prove its package, model/runtime identity, interception, cover, replacement, and cleanup behavior inside the installed QEMU guest.

**Architecture:** Add generic local-extra-package and external-acceptance inputs to the local `omarchy-iso` branch. Build an Arch-native, explicitly local-only demo package from the `omarchy-kids` flake snapshot and an `omarchy-pkgs` recipe; install a dedicated launcher without changing Omarchy's ordinary Chromium defaults. A Kids-owned guest acceptance suite runs the same harmless 17-image fixture and collects machine-readable proof through the existing QEMU harness.

**Tech Stack:** Bash, Nix flakes, Docker, Arch Linux `makepkg`, ArchISO, Rust 2024/Cargo, Chromium/CDP, ONNX Runtime 1.27.1, NudeNet 320n ONNX, QEMU/KVM, OVMF, Hyprland acceptance scripts.

**Spec:** `plans/managed-browser-content-filter.md`

## Global Constraints

- Complete and record the baseline ISO plan before building the Kids ISO.
- Keep every branch and commit local. Never push, fork, open a PR, add or change a remote, upload an ISO/package/model, or otherwise mutate remote state.
- Treat every generated Kids ISO and package as private and non-redistributable: the project has no selected source license and the model's separate license/training-data provenance remain unresolved.
- Keep `mkarchiso` and Arch package construction inside the existing privileged Arch Linux Docker workflow; Nix supplies only host tools and command wrappers.
- Keep the existing `--local-source` behavior compatible and add only generic `omarchy-iso` interfaces. No ISO script or builder variable may contain `kids`, `nudenet`, or the demo package name.
- Build the extra package with dependency installation and normal SHA-256 verification. Do not use `makepkg --nodeps` or `makepkg --skipchecksums` for an extra package.
- Pin ONNX Runtime to 1.27.1 archive SHA-256 `25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af` and NudeNet `320n.onnx` to SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.
- Do not install or enable a service, change `chromium.desktop`, change MIME/default-browser routing, or install a machine-wide Chromium policy. The current `run` command owns one loopback fixture and cannot browse arbitrary URLs.
- The installed desktop entry must be labeled `Managed Browser Filter — Controlled Demo`; it must not present itself as the normal browser or as production parental control.
- Guest proof uses only the harmless deterministic fixture: 17 intercepted, 16 continued, 1 replaced, 0 unresolved, clean shutdown, opaque pre-reveal cover, all safe colors plus placeholder after reveal, and no original flagged color.
- Preserve immutable release ISOs, named VM bases, exported local-package archives, and run evidence. The existing package-download cache remains deliberately mutable: local rebuilds replace same-name cache entries and mirror pruning removes files outside the selected closure.

---

### Task 1: Generic local-extra-package ISO input

**Files in `/home/jake/Projects/omarchy-iso`:**
- Modify: `bin/omarchy-iso-make`
- Modify: `builder/build-iso.sh`
- Modify: `builder/build-omarchy-packages.sh`
- Create: `builder/build-local-package.sh`
- Create: `test/unit/local-package-test.sh`

**Interfaces:**
- Consumes: repeatable `--local-package NAME SOURCE_DIR`, valid only with `--local-source`.
- Consumes: optional `--output-tag TAG`, where `TAG` matches `^[a-z0-9][a-z0-9._-]*$`.
- Produces: container environment `OMARCHY_LOCAL_PACKAGES` as a comma-separated ordered package-name list and read-only mounts `/local-package-sources/NAME`.
- Produces: per-extra-build `OMARCHY_LOCAL_PACKAGE_SRC=/local-package-sources/NAME`.
- Produces: an ISO filename suffixed with `TAG` when set, without changing `OMARCHY_ISO_REF=local` package selection.
- Produces: immutable local-package copies under `release/local-packages/TAG/`; an existing tagged ISO or package-output directory is an error, never an overwrite.

- [ ] **Step 1: Write failing host-parser and builder-contract tests**

Add behavioral shell cases that run the real `omarchy-iso-make` through a fake Docker boundary and run the extracted package-build boundary through fake `makepkg`/package artifacts. Assert:

```text
--local-package demo /path/with spaces
  -> Docker mount /path/with spaces:/local-package-sources/demo:ro
  -> OMARCHY_LOCAL_PACKAGES=demo

--local-package alpha /a --local-package beta /b
  -> preserves alpha,beta order and creates two distinct read-only mounts

--output-tag kids-demo
  -> final output ends in -kids-demo.iso while OMARCHY_ISO_REF remains local
```

Reject a missing `--local-source`, invalid/duplicate names, missing source directories, absent `pkgbuilds/NAME/PKGBUILD`, invalid output tags, and missing option arguments before Docker runs. Package names must match `^[a-z0-9][a-z0-9@._+-]*$`; add explicit rejection cases for `.`, `..`, `../demo`, `demo/child`, and `demo,other`. Prove existing `--local-source OMARCHY PKGS` produces the unchanged mounts and `-local.iso` output when neither new option is present.

For the builder boundary, exercise `builder/build-local-package.sh` with fake `makepkg` and package output directories. Prove the three existing Omarchy packages retain their present `--nodeps --skipchecksums` invocation while every extra uses `--syncdeps --noconfirm --needed --skippgpcheck -f`, receives its exact source directory, and never receives `--nodeps` or `--skipchecksums`. Add an unsigned fake local package whose `.PKGINFO` declares a dependency absent from the main package list; the generated configuration must put `[local-build]` first with `SigLevel = Never`, and the combined-repository resolution test must accept the archive and select both it and that dependency.

- [ ] **Step 2: Run the focused test and capture RED**

Run:

```bash
bash test/unit/local-package-test.sh
```

Expected: failures show `--local-package` and `--output-tag` are unknown and the builder has no extra-package path.

- [ ] **Step 3: Implement strict option parsing and Docker mounts**

Use indexed Bash arrays for names and canonical source paths. Validate after all arguments are parsed so option order does not matter. Pass names as comma-separated data and add one Docker bind mount per source:

```bash
DOCKER_ARGS+=( -e "OMARCHY_LOCAL_PACKAGES=$local_package_csv" )
for index in "${!LOCAL_PACKAGE_NAMES[@]}"; do
  DOCKER_ARGS+=(
    -v "${LOCAL_PACKAGE_PATHS[index]}:/local-package-sources/${LOCAL_PACKAGE_NAMES[index]}:ro"
  )
done
```

Keep `OMARCHY_ISO_REF=local`. Pass a validated nonempty `OMARCHY_ISO_OUTPUT_TAG` into the container when `--output-tag` is set, create `/out/local-packages/$OMARCHY_ISO_OUTPUT_TAG` only if it does not exist, and copy the selected local package archives there before completing the image. Use `${OUTPUT_TAG:-$OMARCHY_ISO_REF}` only in the final host-side rename and fail before `mv` if the tagged destination exists.

- [ ] **Step 4: Build extras and place them in the offline closure**

In `build-omarchy-packages.sh`, split the fixed Omarchy package names from parsed extras. Move the extra build boundary into `builder/build-local-package.sh NAME RECIPE_DIR SOURCE_DIR PKGDEST`; it validates all four arguments, copies the recipe to a fresh work directory, exports `OMARCHY_LOCAL_PACKAGE_SRC`, and invokes dependency-aware verified `makepkg`. For each extra, require both `/omarchy-pkgs/pkgbuilds/$name/PKGBUILD` and `/local-package-sources/$name`, invoke that helper as the unprivileged builder user, and keep exactly one built archive per selected name.

In `build-iso.sh`:

```bash
IFS=',' read -r -a local_extra_packages <<< "${OMARCHY_LOCAL_PACKAGES:-}"
printf '%s\n' "${local_extra_packages[@]}" \
  >> "$build_cache_dir/airootfs/usr/share/omarchy-iso/omarchy-base.packages"
```

Include all local names in the package collection. Before dependency resolution, index every locally built archive in a temporary `[local-build]` file repository placed ahead of the online repositories in a generated pacman configuration. That generated section must set `SigLevel = Never` because these local archives are unsigned, matching the existing final offline repository's ISO-integrity trust boundary. Run `pacman -Syw` and `pacman -S --print` for the complete target set with that combined configuration: pacman must accept the unsigned local archive, select it ahead of a published name, and recursively download any dependency unique to an extra package. Use the exact printed filenames as the prune keep-set, remove the temporary local-build database, then create the final `offline` database as today. Do not merely remove local names from `all_packages` and append their archives afterward; that does not resolve their dependencies. Do not modify the source checkout's package lists.

- [ ] **Step 5: Verify and commit locally**

Run:

```bash
bash test/unit/local-package-test.sh
./test/all
bash -n bin/omarchy-iso-make builder/build-iso.sh builder/build-omarchy-packages.sh builder/build-local-package.sh test/unit/local-package-test.sh
git diff --check
```

Expected: focused contracts, all shell tests, and all Python tests pass. Commit only in the local ISO branch:

```bash
git add bin/omarchy-iso-make builder/build-iso.sh builder/build-omarchy-packages.sh builder/build-local-package.sh test/unit/local-package-test.sh
git commit -m "Support verified local packages in ISO builds"
```

### Task 2: Arch-native controlled-demo package

**Files in `/home/jake/Projects/omarchy-kids`:**
- Modify: `crates/browser-filter/src/browser.rs`
- Create: `packaging/arch/omarchy-kids-browser-filter-demo`
- Create: `packaging/arch/omarchy-kids-browser-filter-demo.desktop`
- Create: `packaging/NOTICES.md`
- Create: `crates/browser-filter/tests/package_assets.rs`

**Files in `/home/jake/Projects/omarchy-pkgs`:**
- Create: `pkgbuilds/omarchy-kids-browser-filter-demo/PKGBUILD`
- Create: `pkgbuilds/omarchy-kids-browser-filter-demo/.omarchy/package.json`

**Interfaces:**
- Consumes: `OMARCHY_LOCAL_PACKAGE_SRC`, the Git-indexed Nix source snapshot mounted by Task 1; intended files must be tracked before evaluation.
- Produces: Arch package `omarchy-kids-browser-filter-demo` version `0.1.0-1` for `x86_64`.
- Produces: `/usr/bin/omarchy-kids-browser-filter-demo`, private executable/runtime files below `/usr/lib/omarchy-kids-browser-filter-demo`, model/extension files below `/usr/share/omarchy-kids-browser-filter-demo`, and the named desktop entry below `/usr/share/applications`.
- Produces: headed JSON fields `onnx_runtime_version` and `model_sha256`, read from the verified detector metadata that actually owns the session.

- [ ] **Step 1: Write failing package-asset tests**

First add a browser summary unit test using detector metadata whose path name and reported runtime version disagree. It must require `onnx_runtime_version == "1.27.1"` from the runtime API metadata and the exact model SHA field; a versioned filename is not the oracle. Then add Rust integration tests that parse the wrapper and desktop entry as their public formats and assert exact behavior:

```text
wrapper exports:
  CHROMIUM_BIN=/usr/bin/chromium
  ORT_DYLIB_PATH=/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1
  NUDENET_MODEL_PATH=/usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx
  OMARCHY_KIDS_EXTENSION_DIR=/usr/share/omarchy-kids-browser-filter-demo/browser-extension
wrapper execs the private binary with subcommand run and forwards all caller arguments

desktop entry:
  Type=Application
  Name=Managed Browser Filter — Controlled Demo
  Exec=omarchy-kids-browser-filter-demo --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash
  Terminal=false
```

Also assert `packaging/NOTICES.md` names the two pinned hashes and states that the resulting package/ISO is private and non-redistributable pending source/model licensing resolution.

- [ ] **Step 2: Run the focused test and capture RED**

Run:

```bash
nix develop -c cargo test -p omarchy-kids-browser-filter \
  browser::tests::headed_summary_uses_detector_reported_identity
nix develop -c cargo test -p omarchy-kids-browser-filter --test package_assets
```

Expected: failure because the summary fields and three package assets do not exist.

- [ ] **Step 3: Carry verified detector identity into the headed summary**

Clone `Detector::metadata()` before moving the detector into the inference worker. Serialize the two strings in every successful headed `ExperimentSummary`:

```rust
pub onnx_runtime_version: String,
pub model_sha256: String,
```

Do not infer either value from a filename or environment variable, and do not add these fields to per-image privacy-safe metrics.

- [ ] **Step 4: Implement the wrapper, desktop entry, and notice**

The wrapper must contain only strict shell setup, the four absolute environment values above, and:

```bash
exec /usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter run "$@"
```

Do not add a service unit, autostart file, browser-default change, policy file, network fetch, or privileged operation.

- [ ] **Step 5: Add the local-only package recipe**

Create a package recipe with:

```bash
pkgname=omarchy-kids-browser-filter-demo
pkgver=0.1.0
pkgrel=1
pkgdesc='Private controlled managed-Chromium filtering demonstrator for Omarchy'
arch=('x86_64')
url='https://omarchy.org'
license=('custom:Omarchy-Kids-Private-Evaluation')
depends=('chromium' 'gcc-libs' 'glibc')
makedepends=('cargo')
source=(
  'onnxruntime-linux-x64-1.27.1.tgz::https://github.com/microsoft/onnxruntime/releases/download/v1.27.1/onnxruntime-linux-x64-1.27.1.tgz'
  '320n.onnx::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/nudenet/320n.onnx'
  'nudenet-LICENSE::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/LICENSE'
  'nudenet-setup.py::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/setup.py'
)
sha256sums=(
  '25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af'
  'c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f'
  '8486a10c4393cee1c25392769ddd3b2d6c242d6ec7928e1414efff7dfb2f07ef'
  'acef07396af96db42374fc2f26799111d0989873f00da5494ac54a0ecf83ff09'
)
```

`prepare()` must fail unless `OMARCHY_LOCAL_PACKAGE_SRC` is a directory, then copy that Git-indexed Nix source snapshot into `$srcdir/omarchy-kids`. Before compilation, run `cargo fetch --locked` while the package builder has network access; Cargo verifies registry checksums from `Cargo.lock`. `build()` then runs `cargo build --frozen --release -p omarchy-kids-browser-filter`. `check()` sets the extracted ORT library and model paths and runs the library plus real inference CLI tests with `--frozen`; it must not launch headed Chromium. `package()` installs only the release binary, wrapper, two-file extension, pinned model, required ONNX Runtime shared libraries/symlinks and its MIT license/notices, desktop entry, project private-evaluation notice, and the pinned NudeNet `LICENSE`/`setup.py` conflict evidence. The sole PKGBUILD license value is the conservative custom private-evaluation designation; `packaging/NOTICES.md` maps ONNX Runtime to MIT and explicitly says neither the Rust project nor model weights are thereby declared MIT or AGPL.

Set `.omarchy/package.json` to:

```json
{
  "source": "local",
  "skip_build": true
}
```

- [ ] **Step 6: Build and inspect the real package in a fresh Arch container**

After staging all new Git-visible source paths, obtain the exact source snapshot with:

```bash
kids_source=$(nix flake archive --json | jq -r .path)
package_evidence=/home/jake/Projects/omarchy-iso/release/local-packages/preflight-$(date -u +%Y%m%d-%H%M%S-%N)
mkdir -p "$package_evidence"
```

Run a fresh `archlinux/archlinux:latest` container with the local recipe mounted at `/recipe:ro`, `$kids_source` at `/source:ro`, `builder/build-local-package.sh` at `/builder/build-local-package.sh:ro`, and `$package_evidence` at `/out`. Inside it, fully upgrade, install `base-devel`, `sudo`, and `cargo`, create the unprivileged builder/sudoers contract used by the ISO build, make `/out` and the helper's work directory writable by `builder`, and invoke:

```bash
runuser -u builder -- /builder/build-local-package.sh \
  omarchy-kids-browser-filter-demo /recipe /source /out
```

Then run `pacman -Qip` and `pacman -Qlp` on the real archive, install it in that disposable container, verify the installed wrapper/desktop/model/extension/runtime/license ownership and modes, and reject every `ldd`/ELF interpreter path containing `/nix/store`. Generate a harmless 1×1 PNG and run the private binary's `infer` subcommand with explicit installed values:

```bash
ORT_DYLIB_PATH=/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1 \
NUDENET_MODEL_PATH=/usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx \
  /usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter infer /tmp/safe.png
```

Preserve the package archive plus command logs in `$package_evidence`.

- [ ] **Step 7: Verify both repositories and commit locally**

In `/home/jake/Projects/omarchy-pkgs`, create local branch `omarchy-kids-local-package` from the clean recorded `master` commit before adding the recipe. Do not configure an upstream. Stage the new Kids assets before any Nix command so the flake snapshot contains them. Run:

```bash
nix run .#check
nix flake check
bash -n packaging/arch/omarchy-kids-browser-filter-demo
```

Run `makepkg --printsrcinfo` for the recipe in the same Arch environment and verify the emitted sources, hashes, dependencies, and `x86_64` architecture. Commit the Kids code/assets and recipe separately on local-only branches:

```bash
cd /home/jake/Projects/omarchy-kids/.worktrees/managed-browser-filter
git add crates/browser-filter/src/browser.rs packaging crates/browser-filter/tests/package_assets.rs
git commit -m "Package the controlled browser demo"

cd /home/jake/Projects/omarchy-pkgs
git add pkgbuilds/omarchy-kids-browser-filter-demo
git commit -m "Add local Omarchy Kids demo recipe"
```

### Task 3: Generic external guest-acceptance hook

**Files in `/home/jake/Projects/omarchy-iso`:**
- Modify: `bin/omarchy-iso-test`
- Modify: `test/unit/host-dependencies-test.sh` only if the real dependency boundary changes
- Create: `test/unit/external-acceptance-test.sh`

**Interfaces:**
- Consumes: optional `--external-acceptance SOURCE_DIR`, requiring executable `SOURCE_DIR/test/acceptance`.
- Produces: guest tree `~/.local/share/omarchy-iso-external-acceptance/test`.
- Produces: guest artifacts `/tmp/omarchy-external-acceptance`, including `acceptance.log`, collected under the existing host `$RUN_DIR`.

- [ ] **Step 1: Write failing end-to-end harness contract tests**

Execute the real acceptance script against fake `tar`, `ssh`, QEMU, and session boundaries. Assert the new option:

- rejects missing arguments and a source without `test/acceptance` before VM work;
- streams only the selected source's `test/` tree into the fixed generic guest path;
- retains the installed product path `OMARCHY_PATH=/usr/share/omarchy`;
- runs the external suite with `OMARCHY_ACCEPTANCE_DIR=/tmp/omarchy-external-acceptance` after the normal shortcut and Omarchy acceptance phases;
- runs through `ssh_session` so the external suite inherits the live user's Wayland, D-Bus, Hyprland, locale, and installed Omarchy environment;
- propagates a nonzero external-suite status; and
- always collects both normal and external artifact directories without weakening cleanup, and fails the external run if the host-side collected `acceptance.log` is absent or empty.

- [ ] **Step 2: Run the focused test and capture RED**

Run:

```bash
bash test/unit/external-acceptance-test.sh
```

Expected: failure because `--external-acceptance` is unknown.

- [ ] **Step 3: Implement the narrow generic hook**

Keep existing `--sync-omarchy` and `--sync-all` behavior unchanged. Add a separate source variable and functions equivalent to:

```bash
sync_external_acceptance() {
  [[ -n $EXTERNAL_ACCEPTANCE_DIR ]] || return 0
  tar -C "$EXTERNAL_ACCEPTANCE_DIR" -cf - test |
    ssh_guest 'mkdir -p .local/share/omarchy-iso-external-acceptance && tar -C .local/share/omarchy-iso-external-acceptance -xf -'
}

run_external_acceptance() {
  [[ -n $EXTERNAL_ACCEPTANCE_DIR ]] || return 0
  ssh_session "mkdir -p /tmp/omarchy-external-acceptance; \
    set -o pipefail; \
    OMARCHY_PATH=/usr/share/omarchy \
    OMARCHY_ACCEPTANCE_DIR=/tmp/omarchy-external-acceptance \
    OMARCHY_ACCEPTANCE_SUDO_PASSWORD=$GUEST_PASSWORD \
    bash .local/share/omarchy-iso-external-acceptance/test/acceptance \
    2>&1 | tee /tmp/omarchy-external-acceptance/acceptance.log"
}
```

Run it in addition to, not instead of, the existing installed-product acceptance. Save its true exit status, always collect `/tmp/omarchy-external-acceptance` into the same timestamped host run directory, and only then combine suite/collection status. When external acceptance was requested, an absent or empty host `$RUN_DIR/omarchy-external-acceptance/acceptance.log` is a harness failure. The Kids suite itself must not exit successfully until its summary and metrics files exist and have passed their assertions, so successful whole-directory collection preserves all three required files.

- [ ] **Step 4: Verify and commit locally**

Run:

```bash
bash test/unit/external-acceptance-test.sh
./test/all
bash -n bin/omarchy-iso-test test/unit/external-acceptance-test.sh
git diff --check
```

Commit only in the local ISO branch:

```bash
git add bin/omarchy-iso-test test/unit/external-acceptance-test.sh
git commit -m "Support external ISO acceptance suites"
```

### Task 4: Kids ISO commands and in-guest proof

**Files in `/home/jake/Projects/omarchy-kids`:**
- Modify: `nix/apps.nix`
- Create: `scripts/kids-iso-build`
- Create: `scripts/kids-iso-test`
- Create: `test/acceptance`
- Create: `test/acceptance.d/browser-filter-demo-test.sh`
- Modify: `crates/browser-filter/tests/workspace_scripts.rs`
- Modify: `README.md`

**Interfaces:**
- Produces: flake apps `kids-iso-build` and `kids-iso-test`.
- Consumes: `OMARCHY_KIDS_PACKAGE_SOURCE`, set by Nix to the Git-indexed Nix source snapshot.
- Consumes: optional `OMARCHY_KIDS_ISO_TAG` for tests; the normal default is `kids-demo-$(date -u +%Y%m%d-%H%M%S-%N)`.
- Produces: `*-kids-demo-*.iso` without overwriting the baseline `*-local.iso` or an earlier Kids ISO.
- Produces: JSON/JSONL guest evidence and acceptance logs under `omarchy-external-acceptance` in the host run directory.

- [ ] **Step 1: Write failing wrapper contracts**

Extend the executable workspace tests with fake sibling commands. Assert `kids-iso-build` invokes exactly:

```text
omarchy-iso-make
  --keep-pkg-cache
  --no-boot-offer
  --local-source OMARCHY_PATH OMARCHY_PKGS_PATH
  --local-package omarchy-kids-browser-filter-demo OMARCHY_KIDS_PACKAGE_SOURCE
  --output-tag OMARCHY_KIDS_ISO_TAG
```

Set `OMARCHY_KIDS_ISO_TAG=kids-demo-contract` in the test and assert that exact argument. Assert `kids-iso-test ISO ARGS...` canonicalizes the ISO, exports `OMARCHY_ISO_MANAGE_HOST_DEPS=0`, and invokes `omarchy-iso-test ISO --external-acceptance OMARCHY_KIDS_PACKAGE_SOURCE ARGS...`. Missing ISO/source contracts must fail before delegation. If `--reuse-base` is absent and `$OMARCHY_ISO_PATH/test-runs/$(basename "$ISO" .iso)/base.qcow2` already exists, the wrapper must refuse to overwrite it and explain that the caller can reuse the base or build a uniquely tagged ISO.

- [ ] **Step 2: Capture wrapper RED**

Run:

```bash
nix develop -c cargo test -p omarchy-kids-browser-filter --test workspace_scripts
```

Expected: failure because both scripts/apps are absent.

- [ ] **Step 3: Implement and expose the two thin wrappers**

Use the existing `resolve-workspace`, OVMF environment, `isoVmInputs`, and `mkApp`. Set `OMARCHY_KIDS_PACKAGE_SOURCE = source` in only these two apps. The build app adds Docker to the existing unit inputs; the test app uses `isoVmInputs`. Do not duplicate builder or QEMU logic in Kids scripts.

- [ ] **Step 4: Write the in-guest acceptance test and capture RED outside the VM**

Create a focused shell runner that discovers the existing Hyprland/Wayland user session, then runs `browser-filter-demo-test.sh` with a 420-second timeout. For RED, execute the browser test directly with controlled temporary `pacman`/session-command fakes and the package absent; it must fail before browser launch with the exact first diagnostic `not ok - omarchy-kids-browser-filter-demo package is installed`.

- [ ] **Step 5: Implement the installed-product assertions**

The guest test must:

1. verify `pacman -Q omarchy-kids-browser-filter-demo` and every installed binary, extension, desktop, model, runtime, and notice path;
2. verify `sha256sum` of the model is exactly `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`;
3. start `omarchy-kids-browser-filter-demo --images 17 --flagged-index 5 --hold-millis 1500 --assert-no-flash --json` in the live user graphical session, capturing stdout to `$OMARCHY_ACCEPTANCE_DIR/browser-filter-summary.json` and stderr to `$OMARCHY_ACCEPTANCE_DIR/browser-filter-metrics.jsonl`;
4. observe a headed Chromium client through `hyprctl -j clients` while the command is running;
5. require exit status 0 and validate with `jq`:

```jq
.intercepted == 17 and
.continued == 16 and
.replaced == 1 and
.unresolved == 0 and
.clean_shutdown == true and
.onnx_runtime_version == "1.27.1" and
.model_sha256 == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f" and
.reveal_latency_millis <= 500 and
.no_flash_assertion.requested_hold_millis == 1500 and
.no_flash_assertion.hold_screenshot_count >= 1 and
.no_flash_assertion.hold_sampled_pixels > 0 and
.no_flash_assertion.cover_rgba == [17,19,24,255] and
.no_flash_assertion.safe_fixture_colors_present == 16 and
.no_flash_assertion.placeholder_color_present == true and
.no_flash_assertion.original_flagged_color_absent == true and
(.dom_images | length) == 17 and
.dom_images[5].rgba == [255,0,255,255]
```

6. require exactly 34 privacy-safe metric records and reject any record whose keys differ from `elapsed_micros`, `fixture_index`, `stage`, and `verdict`;
7. require the summary, metrics, and acceptance log to exist and be nonempty before the suite exits 0; and
8. verify no disposable `omarchy-kids-browser-*` profile remains after exit.

- [ ] **Step 6: Verify and commit locally**

Stage new files, then run:

```bash
nix fmt -- --check
nix run .#check
nix flake check
nix flake show
bash -n scripts/kids-iso-build scripts/kids-iso-test test/acceptance test/acceptance.d/browser-filter-demo-test.sh
git diff --check
```

Update `README.md` with the two commands and the private controlled-demo warning. Commit:

```bash
git add nix/apps.nix scripts test crates/browser-filter/tests/workspace_scripts.rs README.md
git commit -m "Add Kids ISO demo workflow"
```

### Task 5: Build, install, and validate the private Kids ISO

**Files in `/home/jake/Projects/omarchy-kids`:**
- Modify: `docs/experiment-results.md`
- Modify: `plans/managed-browser-content-filter.md`

**Interfaces:**
- Consumes: reviewed local commits from Tasks 1–4 and the completed baseline ISO evidence.
- Produces: a private `*-kids-demo-*.iso`, checksum, build/install durations, reusable guest base, in-VM browser proof, and preserved artifacts.

- [ ] **Step 1: Run fresh preflight evidence**

From the Kids worktree, record exact commits/branches of all four repositories and run:

```bash
nix run .#doctor
nix run .#iso-unit
nix run .#check
nix flake check
```

Expected: all exit 0. Confirm no checkout has unreviewed tracked changes before the multi-gigabyte build.

- [ ] **Step 2: Build the private Kids ISO**

Time:

```bash
nix run .#kids-iso-build
```

Expected: exit 0 and exactly one new `/home/jake/Projects/omarchy-iso/release/*-kids-demo-*.iso` plus its unique `release/local-packages/TAG/` archive directory. Record absolute paths, byte sizes, SHA-256 values, wall duration, and package metadata. Confirm the baseline `*-local.iso` and every earlier tagged Kids ISO remain unchanged.

- [ ] **Step 3: Install it into a fresh reusable VM base**

Time:

```bash
kids_iso=$(find /home/jake/Projects/omarchy-iso/release -maxdepth 1 -type f -name '*-kids-demo-*.iso' -printf '%T@ %p\n' |
  sort -n | tail -1 | cut -d' ' -f2-)
test -n "$kids_iso" && test -f "$kids_iso"
nix run .#kids-iso-test -- "$kids_iso" --install-only --no-preview
```

Expected: the real configurator installs the ISO, the VM reaches the installed system, and a new base qcow2/OVMF pair is preserved beneath `omarchy-iso/test-runs`.

- [ ] **Step 4: Run the installed Kids acceptance proof**

Time the same app and exact ISO with:

```bash
nix run .#kids-iso-test -- "$kids_iso" --reuse-base --no-preview
```

Expected: normal Omarchy shortcut/acceptance checks and the external Kids suite pass. Locate the timestamped run directory and independently re-run the `jq` assertions from Task 4 against the collected `omarchy-external-acceptance/browser-filter-summary.json`; verify ONNX Runtime 1.27.1, the exact model hash, 34 metric lines, and a nonempty `acceptance.log`, and preserve the serial/install/package logs.

- [ ] **Step 5: Record the exact result without broadening the claim**

Update `docs/experiment-results.md` with repositories/commits, commands, ISO/package hashes, runtime/model identity, durations, acceptance JSON, artifact paths, and failures/retries. Update the spec's measured ISO section to say exactly what passed.

The conclusion must say: this proves Arch packaging, offline installation, dedicated launcher execution, and the controlled fixture's interception/cover/replacement/cleanup inside an installed Omarchy VM. It does not prove arbitrary-site browsing, pornography-classifier accuracy, adversarial coverage, a tamper-resistant cover, service supervision, safe default-browser enforcement, or redistribution rights.

- [ ] **Step 6: Fresh verification, local commit, and final review**

Run:

```bash
nix run .#check
nix flake check
git diff --check
git status --short --branch
```

Commit only the documentation:

```bash
git add docs/experiment-results.md plans/managed-browser-content-filter.md
git commit -m "Record installed Kids ISO demo validation"
```

Perform a whole-branch review across the three local branches, fix every Critical/Important finding with fresh regression evidence, rerun the host and ISO checks, and leave all branches, ISOs, VM disks, packages, and evidence local.
