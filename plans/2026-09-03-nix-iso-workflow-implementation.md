# Nix and Omarchy ISO Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide reproducible commands that validate, build, boot, and test Omarchy ISOs from sibling source checkouts while retaining the existing Docker, ArchISO, and QEMU implementation.

**Architecture:** The Omarchy Kids flake supplies pinned host tools and thin runtime wrappers. General environment overrides in a personal fork of `omarchy-iso` remove Arch-host path assumptions without introducing Kids-specific logic; the initial ISO build validates the baseline local-source pipeline but does not yet install the browser experiment.

**Tech Stack:** Nix flakes, Bash, Docker 29, ArchISO/mkarchiso, QEMU/KVM, OVMF, Omarchy ISO test harness.

**Spec:** `plans/managed-browser-content-filter.md`

## Global Constraints

- Keep `mkarchiso` inside the existing privileged Arch Linux Docker container.
- Keep ISO/QEMU commands out of `nix flake check` because they require mutable state, network access, Docker, and KVM.
- Default sibling paths are `../omarchy`, `../omarchy-iso`, and `../omarchy-pkgs`, with environment overrides.
- Always pass `--keep-pkg-cache` and `--no-boot-offer` to unattended local ISO builds.
- Add only generic host-dependency and firmware overrides to `omarchy-iso`; no Omarchy Kids flag.
- Do not claim the browser filter is installed in the ISO until a separate package-integration milestone exists.

---

### Task 1: Gather writable sibling repositories

**Files:**
- External checkout: `../omarchy-iso`
- External checkout: `../omarchy-pkgs`

**Interfaces:**
- Produces: writable GitHub forks `jaketothepast/omarchy-iso` and `jaketothepast/omarchy-pkgs`.
- Produces: each local checkout has `origin` set to the fork and `upstream` set to `omacom/<repo>`.

- [ ] **Step 1: Prove the sibling paths are absent**

Run: `test ! -e ../omarchy-iso && test ! -e ../omarchy-pkgs`

Expected: exit 0. If either path exists, inspect rather than overwrite it.

- [ ] **Step 2: Fork and clone both repositories**

Use `gh repo fork omacom/omarchy-iso --clone=false` and `gh repo fork omacom/omarchy-pkgs --clone=false`, then clone each fork into its sibling path and add canonical `upstream` remotes.

- [ ] **Step 3: Verify provenance**

Run `git remote -v`, `git status --short --branch`, and `gh repo view --json isFork,parent` in both checkouts.

Expected: clean default branches, writable fork origins, and parents owned by `omacom`.

### Task 2: ISO workflow applications and doctor

**Files:**
- Create: `nix/apps.nix`
- Modify: `flake.nix`
- Create: `scripts/resolve-workspace`
- Create: `scripts/doctor`
- Create: `scripts/iso-unit`
- Create: `scripts/iso-build`
- Create: `scripts/iso-test`
- Create: `scripts/iso-integration`

**Interfaces:**
- Consumes: `OMARCHY_PATH`, `OMARCHY_ISO_PATH`, and `OMARCHY_PKGS_PATH` with sibling defaults.
- Produces: flake apps `doctor`, `iso-unit`, `iso-build`, `iso-test`, and `iso-integration`.

- [ ] **Step 1: Write failing shell-contract tests**

Create Rust integration tests that execute `scripts/resolve-workspace` against temporary missing and valid directory trees. Assert missing checkouts fail with exact remediation, valid Git worktrees resolve to canonical paths, and environment overrides win over defaults.

- [ ] **Step 2: Confirm failure**

Run: `cargo test -p omarchy-kids-browser-filter --test workspace_scripts`

Expected: FAIL because scripts do not exist.

- [ ] **Step 3: Implement the resolver and doctor**

`resolve-workspace` exports canonical paths after checking the three expected files: `$OMARCHY_PATH/install/omarchy-base.packages`, `$OMARCHY_ISO_PATH/bin/omarchy-iso-make`, and `$OMARCHY_PKGS_PATH/pkgbuilds/omarchy-dev/PKGBUILD`. `doctor` checks Chromium, ONNX Runtime, Docker server access, writable `/dev/kvm`, both OVMF files, free disk space of at least 40 GiB, and the sibling repository contracts. It reports one line per check and exits non-zero if any required check fails.

- [ ] **Step 4: Implement thin ISO wrappers**

`iso-unit` runs `$OMARCHY_ISO_PATH/test/all`. `iso-build` runs:

```bash
"$OMARCHY_ISO_PATH/bin/omarchy-iso-make" \
  --keep-pkg-cache --no-boot-offer \
  --local-source "$OMARCHY_PATH" "$OMARCHY_PKGS_PATH" "$@"
```

`iso-test` and `iso-integration` require an explicit ISO argument and delegate all remaining arguments. They set `OMARCHY_ISO_MANAGE_HOST_DEPS=0` and both OVMF environment values supplied by Nix.

- [ ] **Step 5: Expose and verify flake apps**

Run:

```bash
nix flake show
nix run .#doctor
nix run .#iso-unit
```

Expected: all apps are listed; doctor and VM-free ISO tests pass.

- [ ] **Step 6: Commit**

```bash
git add flake.nix nix scripts crates/browser-filter/tests
git commit -m "Add reproducible Omarchy ISO workflow commands"
```

### Task 3: Generalize OVMF paths in the ISO harness

**Files in `../omarchy-iso`:**
- Modify: `bin/omarchy-iso-boot`
- Modify: `bin/omarchy-iso-test`
- Modify: `bin/omarchy-iso-test-windows-disk`
- Modify: `test/integration.d/base-test.sh`
- Create or modify: focused tests under `test/unit/`

**Interfaces:**
- Produces: `OMARCHY_VM_OVMF_CODE`, default `/usr/share/edk2/x64/OVMF_CODE.4m.fd`.
- Produces: `OMARCHY_VM_OVMF_VARS_TEMPLATE`, default `/usr/share/edk2/x64/OVMF_VARS.4m.fd`.

- [ ] **Step 1: Write failing contract tests**

Add a source-level test that passes temporary firmware paths through both environment variables and asserts every harness entry point uses the overrides rather than a literal Arch path. Assert unset variables retain the two existing Arch defaults.

- [ ] **Step 2: Confirm failure**

Run: `./test/all`

Expected: the new firmware-override test fails on hardcoded paths.

- [ ] **Step 3: Implement consistent overrides**

At each entry point define:

```bash
OVMF_CODE=${OMARCHY_VM_OVMF_CODE:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
OVMF_VARS_TEMPLATE=${OMARCHY_VM_OVMF_VARS_TEMPLATE:-/usr/share/edk2/x64/OVMF_VARS.4m.fd}
```

Use only these variables in existence checks, copies, and QEMU arguments.

- [ ] **Step 4: Verify and commit in the ISO repository**

Run: `./test/all`

Expected: all ISO unit tests pass.

```bash
git add bin test
git commit -m "Allow alternate OVMF firmware paths"
```

### Task 4: Make ISO host dependency management optional

**Files in `../omarchy-iso`:**
- Modify: `bin/omarchy-iso-boot`
- Modify: `bin/omarchy-iso-test`
- Modify: `bin/omarchy-iso-test-windows-disk`
- Modify: `test/integration.d/base-test.sh`
- Create or modify: focused tests under `test/unit/`

**Interfaces:**
- Produces: `OMARCHY_ISO_MANAGE_HOST_DEPS`, default `1`; `0` skips package mutation but validates dependencies.

- [ ] **Step 1: Write failing dependency-mode tests**

Run each dependency setup function with a fake `omarchy-pkg-add`. Assert default mode calls it with the existing package list. Assert mode `0` never calls it, succeeds when required commands/files exist, and reports every missing command or firmware path before exiting non-zero.

- [ ] **Step 2: Confirm failure**

Run: `./test/all`

Expected: the new tests fail because the mode does not exist.

- [ ] **Step 3: Implement one shared helper**

Create a sourced helper under `bin/` or `test/helpers/` that validates the mode is exactly `0` or `1`, performs current `omarchy-pkg-add` behavior in mode `1`, and checks command/file dependencies in mode `0`. Reuse it from all four entry points; do not add PATH shims.

- [ ] **Step 4: Verify and commit in the ISO repository**

Run: `./test/all`

Expected: existing behavior and Nix-managed mode both pass.

```bash
git add bin test
git commit -m "Support externally managed ISO test dependencies"
```

### Task 5: Build and smoke-test a baseline local-source ISO

**Files:**
- Modify: `docs/experiment-results.md`

**Interfaces:**
- Consumes: the three sibling checkouts, Docker, KVM, and Nix-provided OVMF.
- Produces: baseline ISO path, checksum, build duration, VM-free test outcome, and boot/install smoke outcome.

- [ ] **Step 1: Verify readiness**

Run:

```bash
nix run .#doctor
nix run .#iso-unit
```

Expected: both exit 0.

- [ ] **Step 2: Build the ISO**

Run: `nix run .#iso-build`

Expected: a new `../omarchy-iso/release/*-local.iso` exists and `sha256sum` succeeds.

- [ ] **Step 3: Run install-only smoke**

Run: `nix run .#iso-test -- ../omarchy-iso/release/<exact-local-iso-name> --install-only --no-preview`

Expected: the real ISO installs and produces a reusable base image.

- [ ] **Step 4: Run acceptance against the local Omarchy checkout**

Run: `nix run .#iso-test -- ../omarchy-iso/release/<exact-local-iso-name> --reuse-base --sync-omarchy ../omarchy --no-preview`

Expected: the existing in-guest Omarchy acceptance suite completes; record failures honestly rather than modifying unrelated Omarchy behavior.

- [ ] **Step 5: Document and commit results**

Record exact commits, ISO checksum, durations, and test results in `docs/experiment-results.md`.

```bash
git add docs/experiment-results.md
git commit -m "Record reproducible baseline ISO validation"
```

