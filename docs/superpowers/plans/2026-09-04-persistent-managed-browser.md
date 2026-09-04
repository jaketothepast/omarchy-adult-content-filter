# Persistent Managed Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent, single-tab, host-side Chromium mode that fail-closes ordinary image-response classification and remains open for a person to drive.

**Architecture:** A new `managed` module owns the manual CDP event loop and privacy-safe summary while reusing crate-private detector-worker, response, decoder, and cleanup primitives from the proven fixture controller. Chromium uses a fresh profile and document-start cover, supervises one page, closes additional page targets, resolves every image response once, and remains active until browser close or Ctrl-C.

**Tech Stack:** Rust 2024, Tokio, chromiumoxide/CDP Fetch and Target domains, ONNX Runtime through `ort`, pinned NudeNet model, Clap, Nix.

**Spec:** `docs/superpowers/specs/2026-09-04-persistent-managed-browser-design.md`

## Global Constraints

- Existing `run`, benchmark, inference, ISO, and package behavior must remain unchanged.
- The manual mode owns one supervised page; additional page targets are closed or the session terminates.
- Only successfully classified `Policy::Allow` image bodies and bodyless redirects continue; every classification or deadline failure is replaced.
- Acquisition and inference deadlines are five seconds; launch and cleanup deadlines are thirty seconds; total session lifetime is unbounded.
- No URL, response bytes, page title, path, or browsing history may be written to metrics or summary output.
- Every production behavior starts with a focused failing test and observed RED.
- All changes and commits stay local; do not update the ISO/package or contact remotes.

---

### Task 1: Manual safety model and configuration

**Files:**
- Create: `crates/browser-filter/src/managed.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Test: inline `managed::tests`

**Interfaces:**
- Consumes: `Detector`, `DetectorMetadata`, `InferenceReport`, `Policy`, `Verdict`, `RequestId`.
- Produces: `ManagedBrowserConfig::new(...) -> Result<Self>`, `ManagedBrowserSummary`, `ResponsePlan`, `Classification`, `should_reveal(load_seen, unresolved)`, and `target_action(primary, target_info)`.

- [ ] **Step 1: Write configuration RED tests**

  Add tests that construct a new empty profile and assert acceptance of `None`/HTTP/HTTPS start URLs, rejection of non-HTTP(S) start URLs, default-profile paths, nonempty profiles, missing Chromium/extension manifest, and zero or over-limit timeouts.

- [ ] **Step 2: Run the focused test and verify RED**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter managed::tests::config -- --nocapture`; expect unresolved `ManagedBrowserConfig` symbols.

- [ ] **Step 3: Implement minimal validated configuration**

  Add a config containing `start_url: Option<Url>`, Chromium/profile/extension paths, five-second acquisition/inference deadlines, and thirty-second lifecycle deadline. Share or extract the existing profile/tool/extension validation without weakening fixture URL/image-count rules.

- [ ] **Step 4: Add and observe decision-model RED tests**

  Assert that only bodyless redirects plan `ContinueRedirect`, only status 200 without a response error plans `Classify`, every other status/error plans `ReplaceFailedClosed`, inference/policy allow produces `Allow`, explicit classes produce `ReplaceExplicit`, and inference error produces `ReplaceFailedClosed`. Assert reveal requires both a load event and zero unresolved pauses. Assert only a non-primary `page` target is closed.

- [ ] **Step 5: Implement the pure decision model and verify GREEN**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter managed::tests -- --nocapture`; expect all new model/config tests to pass and existing policy tests to stay green.

- [ ] **Step 6: Commit the model**

  Stage only `managed.rs` and `lib.rs`; commit locally as `Model fail-closed manual browsing`.

### Task 2: Persistent CDP controller

**Files:**
- Modify: `crates/browser-filter/src/browser.rs`
- Modify: `crates/browser-filter/src/managed.rs`
- Test: inline tests in both modules

**Interfaces:**
- Consumes from `browser`: crate-private `HeadedDetectorSession::{start,detect,shutdown,metadata}`, Fetch enable/continue/replace/body-decode helpers, and exhaustive `cleanup_browser`.
- Produces: `ManagedBrowser<W>::new(detector, policy, writer)` and `ManagedBrowser::run(config) -> Result<ManagedBrowserSummary>`.

- [ ] **Step 1: Write fail-closed settlement RED tests**

  Use a small injected decision seam to prove successful allow calls only `continueResponse`; explicit and failed classification call only `fulfillRequest`; counters and ledger change only after successful CDP resolution; failure to fulfill leaves the ledger unresolved and returns fatal error; duplicate request IDs fail.

- [ ] **Step 2: Run the focused RED**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter managed::tests::settlement -- --nocapture`; expect missing settlement/controller symbols.

- [ ] **Step 3: Expose the minimal proven primitives and implement response processing**

  Change only required items in `browser.rs` to `pub(crate)`. In `managed.rs`, implement status planning, bounded body acquisition/decoding/inference, fail-closed conversion, exact one CDP resolution, counters, and ledger removal. Do not route expected classification failures into the session error path.

- [ ] **Step 4: Write event-loop RED tests**

  Through an orchestration seam, prove load events reveal only with zero unresolved responses, a later navigation waits for its own load, Ctrl-C and primary-stream closure stop normally, an additional page is closed, failure to close an additional page is fatal, and no error path skips browser/worker cleanup.

- [ ] **Step 5: Implement the one-page event loop**

  Launch headed Chromium with fresh profile, extension, disabled cache, and existing sandbox behavior. Create/configure the primary `about:blank` page, close startup extras, optionally issue initial navigation, then select over Fetch pauses, load events, target-created events, and Ctrl-C. Reveal with the existing readiness attribute. Aggregate run, worker, metric/output, and Chromium cleanup errors.

- [ ] **Step 6: Verify the complete controller GREEN**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter managed::tests browser::tests -- --nocapture`; then `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 7: Commit the controller**

  Stage `browser.rs` and `managed.rs`; commit locally as `Supervise persistent managed Chromium`.

### Task 3: CLI, Nix app, and documentation

**Files:**
- Modify: `crates/browser-filter/src/main.rs`
- Modify: `nix/apps.nix`
- Modify: `README.md`
- Test: `crates/browser-filter/src/main.rs`

**Interfaces:**
- Consumes: `ManagedBrowser`, `ManagedBrowserConfig`, and environment variables already supplied by the Nix wrapper.
- Produces: `browse { --url <http-or-https>, --json }` and Nix app `.#browse`.

- [ ] **Step 1: Write CLI RED**

  Assert `browse`, optional `--url`, and `--json` parse into the new command without changing `run`. Assert malformed/non-HTTP URLs fail during config construction.

- [ ] **Step 2: Run and observe RED**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter browse_cli -- --nocapture`; expect the missing variant.

- [ ] **Step 3: Implement the CLI and output**

  Create the disposable profile, build the config with the exact 5/5/30-second deadlines, run the controller, remove the profile, and emit either JSON or a concise counts line. Add `browse` to `nix/apps.nix` with the same wrapped runtime inputs as `run`.

- [ ] **Step 4: Document the manual command and limitations**

  Add the exact command, one-tab/fail-closed behavior, deadline values, Ctrl-C/close behavior, and explicit unsupported rendering/bypass surfaces to `README.md`. Do not call it production child safety.

- [ ] **Step 5: Verify and commit**

  Run `nix run .#check`, `nix flake check`, `nix flake show`, and `git diff --check`. Commit the scoped CLI/app/docs change locally as `Expose host managed browsing`.

### Task 4: Real headed smoke, self-review, and user launch

**Files:**
- Modify only if a smoke-discovered defect has a new failing regression test.
- Record: `docs/experiment-results.md`

**Interfaces:**
- Consumes: `nix run .#browse`.
- Produces: real host evidence and a live user-driven managed Chromium process.

- [ ] **Step 1: Start a harmless loopback smoke origin**

  Use a temporary loopback-only HTTP server containing a page with one valid PNG and one response whose bytes are deliberately undecodable as an image. Record its ephemeral port; do not use explicit imagery or an external site.

- [ ] **Step 2: Run the real managed browser smoke**

  Start `nix run .#browse -- --url http://127.0.0.1:<port> --json` in a persistent terminal, wait for both image decisions, send Ctrl-C, and require a clean summary with one continued safe image, one failed-closed replacement, zero unresolved pauses, zero extra page targets, and clean shutdown/profile removal.

- [ ] **Step 3: Self-review the complete range**

  Trace configuration, navigation cover, status/error policy, deadlines, pause ledger, additional-target handling, cleanup, output privacy, CLI, and current fixture non-regression. Fix only evidence-backed findings through new RED/GREEN cycles.

- [ ] **Step 4: Run final verification**

  Run `nix run .#check`, `nix flake check`, the controlled `nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json`, shell/process/profile cleanup checks, and `git diff --check`. Append the honest smoke result and limitations to `docs/experiment-results.md`; commit locally as `Validate host managed browsing`.

- [ ] **Step 5: Launch for the user**

  Verify no prior `omarchy-kids-browser-` Chromium exists, then start `nix run .#browse` in a persistent PTY. Leave it running, report the session and exact shutdown method, and do not navigate on the user's behalf.
