# Bundled Omarchy Adult Content Filter Plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish one Omarchy marketplace plugin repository that contains and supervises the complete managed adult-content-filter browser runtime.

**Architecture:** A kept Omarchy `service` owns one native supervisor process and a `bar-widget` delegates launch/stop to it. The repository bundles the reviewed x86-64 supervisor, ONNX Runtime, NudeNet model, adult-domain policy, cover extension, licenses, and checksum manifest; only the system Chromium executable remains external.

**Tech Stack:** Rust 2024, Bash 5, Quickshell QML, Nix, Arch `makepkg`, Chromium CDP, ONNX Runtime 1.27.1, GitHub/Omarchy plugin marketplace.

**Spec:** `docs/superpowers/specs/2026-09-04-bundled-omarchy-plugin-design.md`

## Global Constraints

- The permanent plugin ID is `io.github.jaketothepast.adult-content-filter`.
- The plugin repository contains exactly one root `manifest.json`.
- The repository must run after `omarchy plugin add` and enablement without a separate package installation.
- The marketplace runtime supports x86-64 Omarchy only and uses `/usr/bin/chromium`.
- Runtime code must not invoke `sudo`, `pkexec`, a package manager, systemd, curl-to-shell, or mutable remote code.
- Safety failures stop before navigation or replace the affected response; they do not silently continue.
- The existing browser filtering and privacy contracts remain unchanged.
- The source and plugin code use AGPL-3.0; upstream component notices remain component-specific.
- The Omarchy, ISO, and package sibling repositories remain local and unpushed.
- The final marketplace issue is created only after the owner approves its exact title, body, and checklist.

---

### Task 1: Omarchy plugin lifecycle and UI

**Files:**
- Create: `manifest.json`
- Create: `Service.qml`
- Create: `BarWidget.qml`
- Create: `plugin/RuntimeModel.js`
- Create: `plugin/tests/runtime-model-test.js`
- Create: `crates/browser-filter/tests/plugin_assets.rs`
- Modify: `crates/browser-filter/Cargo.toml`

**Interfaces:**
- Consumes: Omarchy-injected `manifest`, `shell`, and bar properties.
- Produces: `Service.qml` methods `launch()`, `stop()`, and `focus()` plus observable `running`, `lastExitCode`, and `lastError` properties.

- [ ] **Step 1: Write failing manifest and lifecycle tests**

Add Rust asset tests that require the exact ID, `service` + `bar-widget` kinds, `keepLoaded: true`, relative entry points, and a widget that obtains `shell.serviceFor(manifest.id)` rather than constructing a Chromium command. Add a Node state-model test covering idle launch, duplicate launch/focus, clean exit, failed exit, and stop.

```rust
#[test]
fn manifest_declares_one_kept_service_and_widget() {
    let manifest: serde_json::Value = serde_json::from_str(&read("manifest.json")).unwrap();
    assert_eq!(manifest["id"], "io.github.jaketothepast.adult-content-filter");
    assert_eq!(manifest["kinds"], serde_json::json!(["service", "bar-widget"]));
    assert_eq!(manifest["keepLoaded"], true);
}
```

- [ ] **Step 2: Run the focused tests and capture RED**

Run: `nix develop -c cargo test -p omarchy-kids-browser-filter --test plugin_assets`

Expected: FAIL because the root manifest and QML entry points do not exist.

- [ ] **Step 3: Implement the minimal plugin components**

Create the manifest and QML components. `Service.qml` owns one `Process` with command `[manifest.__sourceDir + "/bin/omarchy-adult-content-filter"]`; `BarWidget.qml` resolves the matching service and calls its methods. Keep transition decisions in `RuntimeModel.js` so they can be behavior-tested without a live shell.

- [ ] **Step 4: Run focused GREEN and mutation checks**

Run the Rust and Node tests. Temporarily mutate the widget to launch the wrapper directly and invert the duplicate-launch transition; verify the tests fail, then restore.

- [ ] **Step 5: Commit**

```bash
command git add manifest.json Service.qml BarWidget.qml plugin/RuntimeModel.js plugin/tests/runtime-model-test.js crates/browser-filter/tests/plugin_assets.rs crates/browser-filter/Cargo.toml
command git commit -m "Add Omarchy plugin supervision surface"
```

### Task 2: Repository-local fail-closed runtime boundary

**Files:**
- Create: `bin/omarchy-adult-content-filter`
- Create: `runtime/SHA256SUMS`
- Create: `tests/plugin-runtime-contract.sh`
- Modify: `nix/apps.nix`
- Modify: `flake.nix`

**Interfaces:**
- Consumes: repository-relative files under `runtime/` and `XDG_RUNTIME_DIR`.
- Produces: a zero-argument wrapper that runs one native supervisor with exact environment paths and one per-user lock.

- [ ] **Step 1: Write the failing wrapper contract**

The test must construct fake runtime payloads and commands, then prove argument/root rejection, unsafe runtime-directory rejection, checksum failure before execution, symlink/path-escape rejection, exact relative environment values, and non-blocking duplicate-instance behavior.

```bash
run_wrapper --unexpected
assert_status 64
assert_file_empty "$fixture/chromium-called"

corrupt "$fixture/plugin/runtime/share/models/320n.onnx"
run_wrapper
assert_status 78
assert_file_empty "$fixture/supervisor-called"
```

- [ ] **Step 2: Run focused RED**

Run: `bash tests/plugin-runtime-contract.sh`

Expected: FAIL because the repository-local wrapper and checksum validation do not exist.

- [ ] **Step 3: Implement the wrapper**

Use Bash 5 strict mode, fixed `/usr/bin` PATH, `umask 077`, canonical plugin-root validation, an allowlisted checksum file, stable ONNX symlink validation, owner/mode checks for `XDG_RUNTIME_DIR`, and `flock -n` on `$XDG_RUNTIME_DIR/omarchy-adult-content-filter/instance.lock`. Export only the existing supervisor environment variables with repository-relative payload paths and execute `runtime/bin/omarchy-adult-content-filter browse --json`.

- [ ] **Step 4: Expose the test in Nix checks and verify GREEN**

Add the wrapper test to the existing check closure with `bash`, `coreutils`, and `util-linux`. Run the focused shell test and the relevant Rust asset test. Mutate one checksum and the lock command to prove the tests reject both regressions.

- [ ] **Step 5: Commit**

```bash
command git add bin/omarchy-adult-content-filter runtime/SHA256SUMS tests/plugin-runtime-contract.sh nix/apps.nix flake.nix
command git commit -m "Enforce the bundled plugin runtime boundary"
```

### Task 3: Reproducible bundled payload

**Files:**
- Create: `packaging/arch/PKGBUILD`
- Create: `scripts/build-plugin-bundle`
- Create: `tests/plugin-bundle-contract.sh`
- Create binary/runtime assets beneath: `runtime/bin/`, `runtime/lib/`, `runtime/share/`
- Modify: `packaging/NOTICES.md`
- Modify: `tests/managed-package-recipe-contract.sh`

**Interfaces:**
- Consumes: exact Git-indexed source, pinned ONNX Runtime/model/domain inputs, and an Arch build environment.
- Produces: the allowlisted `runtime/` tree and regenerated `runtime/SHA256SUMS` at the same Git commit.

- [ ] **Step 1: Write failing bundle and recipe tests**

Require the recipe to live in this repository, identify this GitHub repository, use one runtime version variable, pin all remote inputs, license original code as AGPL-3.0, and produce exactly the runtime allowlist. Require the bundle script to reject a dirty source tree, multiple packages, wrong package identity, extra paths, symlink escapes, unresolved ELF dependencies, and mismatched supervisor/model/runtime identities.

```bash
expected=(
  runtime/bin/omarchy-adult-content-filter
  runtime/lib/libonnxruntime.so.1.27.1
  runtime/share/models/320n.onnx
  runtime/share/policies/adult-domains.hosts
)
assert_exact_bundle_allowlist "${expected[@]}"
```

- [ ] **Step 2: Run focused RED**

Run: `bash tests/plugin-bundle-contract.sh`

Expected: FAIL because the local recipe, bundle builder, and payload are absent.

- [ ] **Step 3: Consolidate the package recipe and bundle builder**

Move the reviewed PKGBUILD contract into `packaging/arch/PKGBUILD`, update it to the public repository identity and AGPL source license, and make `scripts/build-plugin-bundle` extract only the runtime files from one verified package into a staging directory. Verify bytes and symlinks before atomically replacing `runtime/` and its checksum manifest.

- [ ] **Step 4: Build a fresh payload and verify GREEN**

Run the clean Arch build against the exact source commit. Run package metadata, Qip/Qlp, mode/ownership, ELF/SONAME, real 1x1 inference, payload allowlist, and wrapper smoke checks. Preserve the build log and hashes beneath an ignored evidence directory.

- [ ] **Step 5: Commit**

```bash
command git add packaging/arch/PKGBUILD scripts/build-plugin-bundle tests/plugin-bundle-contract.sh tests/managed-package-recipe-contract.sh packaging/NOTICES.md runtime
command git commit -m "Bundle the reviewed filtering runtime"
```

### Task 4: Public documentation, licenses, and preview

**Files:**
- Create: `LICENSE`
- Create: `THIRD_PARTY_NOTICES.md`
- Create: `preview.png`
- Modify: `README.md`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: final plugin manifest/runtime contract and verified evidence.
- Produces: marketplace-ready installation, operation, removal, limitation, and licensing documentation.

- [ ] **Step 1: Write failing public-release documentation tests**

Extend `plugin_assets.rs` to require the root files, exact `omarchy plugin add` and removal commands, x86-64/system-Chromium dependencies, bundled-executable disclosure, AGPL scope, upstream licenses, browser-only limitations, and preview bounds.

- [ ] **Step 2: Run focused RED**

Run: `nix develop -c cargo test -p omarchy-kids-browser-filter --test plugin_assets`

Expected: FAIL because the public license, notices, preview, and final marketplace instructions are absent.

- [ ] **Step 3: Write the release surface**

Add AGPL-3.0, third-party notices, concise README installation/removal instructions, safety behavior, external dependencies, and explicit anti-tamper limitations. Add one root preview showing the Omarchy widget and managed browser without explicit imagery.

- [ ] **Step 4: Verify GREEN and scan for secrets/private paths**

Run the focused tests, image metadata inspection, `git grep` for private evaluation language and absolute local workspace paths, and a tracked-file size inventory. Remove internal-only evidence from the public tracked surface if it is not needed to build or review the plugin.

- [ ] **Step 5: Commit**

```bash
command git add LICENSE THIRD_PARTY_NOTICES.md preview.png README.md .gitignore crates/browser-filter/tests/plugin_assets.rs
command git commit -m "Prepare the plugin for public distribution"
```

### Task 5: Full local and marketplace validation

**Files:**
- Modify only if a verified defect is found: files owned by Tasks 1–4.
- Record ignored evidence beneath: `.superpowers/sdd/2026-09-04-bundled-plugin-publication/`

**Interfaces:**
- Consumes: final tracked repository snapshot.
- Produces: exact clean-commit evidence for local functionality and marketplace compatibility.

- [ ] **Step 1: Run complete local gates**

Run:

```bash
nix run .#check
nix flake check
bash tests/plugin-runtime-contract.sh
bash tests/plugin-bundle-contract.sh
command git diff --check
```

- [ ] **Step 2: Run the official marketplace validator and security baseline**

Check out the official marketplace validator at a recorded commit in a disposable directory and scan the exact plugin tree. Require structural validation success, no selectively blocking finding, and only the disclosed `bundled-executable-binary` review capability.

- [ ] **Step 3: Run headed host and installed Omarchy validation**

Launch through the QML service boundary in a disposable Omarchy environment, then run the controlled headed proof. Build one unique ISO from the exact source commit only if the bundled bytes differ from the last installed proof; otherwise reuse the exact matching proof. Require normal Omarchy acceptance, plugin enablement, widget/service launch, filtering summary, privacy metrics, and zero process/profile residue.

- [ ] **Step 4: Fix only evidence-backed defects using RED/GREEN**

For each failure, preserve the failing output, add the smallest focused regression, implement the fix, rerun the focused test, and repeat all affected final gates.

- [ ] **Step 5: Commit final verification documentation**

```bash
command git add README.md docs/experiment-results.md plans/managed-browser-content-filter.md
command git commit -m "Record bundled plugin validation"
```

### Task 6: Publish the standalone repository and prepare submission

**Files:**
- No new product files unless the remote or marketplace validator identifies an exact defect.
- Create temporary submission body: `/tmp/omarchy-adult-content-filter-submission.md`

**Interfaces:**
- Consumes: clean, fully verified local `HEAD`.
- Produces: public `jaketothepast/omarchy-adult-content-filter` repository and an owner-approved marketplace issue.

- [ ] **Step 1: Recheck identity, scope, auth, and ID uniqueness**

Confirm the working tree is clean, GitHub CLI is authenticated as `jaketothepast`, no repository with the destination name already exists, and the plugin ID is absent from active and retired marketplace entries.

- [ ] **Step 2: Create and push the public repository**

Create `jaketothepast/omarchy-adult-content-filter` as public, add it as `origin`, and push local `HEAD` to remote `main`. Verify the public root manifest, README, license, preview, default branch, and exact remote commit. Do not push any sibling repository or deferred guardian branch.

- [ ] **Step 3: Run validation against the public URL**

Repeat the official structure/baseline checks against the exact public commit and confirm the repository clone contains the same tracked tree and payload hashes.

- [ ] **Step 4: Prepare the exact marketplace issue**

Use title `[Plugin]: Omarchy Adult Content Filter`, category `System`, tags `security, launcher, ai`, the public root URL, and maintainer notes disclosing the x86-64 bundled native supervisor, system Chromium dependency, browser-only scope, and expected manual binary review. Preserve all six official headings and five exact checklist items.

- [ ] **Step 5: Obtain required owner confirmation and submit**

Show the exact completed title/body to the owner and ask them to confirm every checklist statement. Only after explicit approval, create the issue in `omacom/omarchy-plugin-marketplace`. Report the public repository URL, issue URL, exact submitted commit, automated validation state, and any maintainer action still required.
