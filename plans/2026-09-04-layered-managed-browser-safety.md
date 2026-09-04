# Layered Managed-Browser Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the driveable managed Chromium prototype into a layered, fail-closed filter that blocks pinned known-adult domains before network transfer, forces supported search/video restrictions, and retains local NudeNet image classification plus page-scoped linked-video blocking as its content layer.

**Architecture:** The browser loads one immutable adult-domain ruleset at startup and refuses to launch if it cannot validate it. Chromium Fetch interception receives every HTTP(S) request at the request stage before it is sent: blocked hosts are failed immediately, Google searches are rewritten to SafeSearch, YouTube requests receive the strict restricted-mode header, and all other requests continue unchanged. Image responses then pause a second time at the existing response-stage classifier. Any image replacement keeps the current document media-tainted, preventing video playback and activation of controls connected to the replaced thumbnail. All decisions remain local and summaries expose aggregate counts only.

**Tech Stack:** Rust 2024, Tokio, chromiumoxide/CDP Fetch, `url`, pinned StevenBlack porn-only hosts data, ONNX Runtime 1.27.1, NudeNet 320n, Clap, Nix, Arch `makepkg`, Omarchy ISO/QEMU acceptance.

**Specs:** `plans/managed-browser-content-filter.md`, `docs/superpowers/specs/2026-09-04-persistent-managed-browser-design.md`

## Fixed Inputs and Safety Boundary

- Ruleset source: StevenBlack `hosts`, commit `2bb49d741a2c9b922b0ed59be6c28ce543bed81b`, `alternates/porn-only/hosts`.
- Ruleset SHA-256: `a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514` (`sha256-pRLCgV/mEvpO7ox7Hi2rF/ejkBZInqs+O7zEB+209RQ=`).
- Expected category entries: 76,767; comments and blank lines do not count.
- The ruleset is fetched only during reproducible Nix/package construction. Browsing never updates or contacts a list provider.
- Matching is case-insensitive and label-aware: an entry blocks itself and subdomains, but never a mere string suffix.
- Request-stage interception is the managed-browser equivalent of a DNS denylist: it stops matching requests before Chromium sends them, without changing the host machine's resolver.
- Non-HTTP(S) schemes receive no implied safety claim. Malformed HTTP(S) URLs, missing hosts, list read/parse failures, CDP timeouts, or unresolved request-stage pauses terminate the browser fail closed.
- Google SafeSearch enforcement follows Chromium's own `safe=active` and `ssui=on` behavior. YouTube enforcement follows Chromium's `YouTube-Restrict: Strict` behavior.
- Summary output contains identities and counts only—never URLs, domains, queries, titles, headers, bodies, filesystem paths, or browsing history.
- No remote mutations, pushes, forks, PRs, or uploads. Preserve all existing ISO/package/VM evidence and the currently running user-driven browser.

---

### Task 1: Bounded offline domain policy

**Files:**
- Create: `crates/browser-filter/src/domain_policy.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Test: inline `domain_policy::tests`

**Interface:** `DomainPolicy::load(path) -> Result<Self>`, `DomainPolicy::contains_host(host) -> bool`, `DomainPolicy::entry_count() -> usize`.

- [ ] **Step 1: Write parser and matching RED tests**

  Cover hosts-file comments/blank lines, a single host per `0.0.0.0` line, multiple hosts on one valid line, lowercase normalization, one trailing root dot, exact match, subdomain match, suffix-boundary rejection, IP-host non-match, duplicate removal, and the exact expected entry count fixture contract.

- [ ] **Step 2: Run the focused RED**

  Run `nix develop -c cargo test -p omarchy-kids-browser-filter domain_policy::tests -- --nocapture`; require unresolved module/type failures before production code exists.

- [ ] **Step 3: Implement the minimal bounded loader**

  Read at most 4 MiB, permit only recognized null-route IP plus valid DNS names, reject malformed non-comment records, require at least one domain, cap unique entries at 250,000, store normalized names in a `HashSet`, and implement label-by-label suffix lookup without allocation during matching.

- [ ] **Step 4: Prove fail-closed input behavior**

  Add RED/GREEN cases for missing file, oversize file, invalid UTF-8, invalid IP field, invalid/empty host, IP literals, and all-invalid input. Mutate label-boundary matching and the malformed-line rejection once each; require a focused failure, then restore.

- [ ] **Step 5: Verify the real pinned list**

  Add a Nix-backed integration test that loads `OMARCHY_KIDS_BLOCKLIST_PATH`, asserts exactly 76,767 entries, and checks representative synthetic exact/subdomain/non-suffix behavior without printing real list entries.

- [ ] **Step 6: Commit locally**

  Run focused tests, Clippy, rustfmt, and `git diff --check`; commit only the domain-policy slice as `Load pinned adult-domain policy`.

### Task 2: Pure request policy and search restrictions

**Files:**
- Create: `crates/browser-filter/src/request_policy.rs`
- Modify: `crates/browser-filter/src/lib.rs`
- Test: inline `request_policy::tests`

**Interface:** `RequestPolicy::evaluate(&Url, headers) -> RequestDecision`, where decisions are block, continue unchanged, continue with rewritten URL, or continue with strict YouTube headers.

- [ ] **Step 1: Write domain-decision RED tests**

  Prove a listed root and subdomain block across documents, images, scripts, XHR, media, and redirects; an unrelated suffix does not block; safe HTTP(S) traffic continues; malformed HTTP(S) input fails closed; and loopback remains available for controlled acceptance fixtures.

- [ ] **Step 2: Implement domain decisions and verify GREEN**

  Keep resource type out of the allow/block decision. Return only privacy-safe decision variants; never retain or serialize the URL.

- [ ] **Step 3: Write Google SafeSearch RED tests**

  Cover Google search/home/safesearch paths and Google subdomains, existing conflicting `safe`/`ssui` values, fragments, duplicate query keys, nonstandard ports, lookalike domains, and unrelated Google services. Require exactly one `safe=active` and `ssui=on` without altering other query values.

- [ ] **Step 4: Implement Google rewriting and verify GREEN**

  Match Chromium's documented scope narrowly; preserve scheme, authority, path, unrelated query pairs, and fragment. Apply the domain denylist before rewrite.

- [ ] **Step 5: Write strict YouTube RED tests**

  Cover `youtube.com` and subdomains, existing mixed-case header replacement, preservation of every unrelated header, nonstandard-port and lookalike rejection, and non-YouTube requests. Require one `YouTube-Restrict: Strict` header.

- [ ] **Step 6: Implement strict YouTube handling and mutation checks**

  Clone request headers only when the YouTube rule applies. Mutate the Google host boundary and YouTube header value; require failures, restore, then run all request-policy tests.

- [ ] **Step 7: Commit locally**

  Run focused tests, Clippy, rustfmt, and diff checks; commit as `Define layered browser request policy`.

### Task 3: Pre-network CDP enforcement

**Files:**
- Modify: `crates/browser-filter/src/browser.rs`
- Modify: `crates/browser-filter/src/managed.rs`
- Test: inline tests in both modules

**Interfaces:** A managed-only Fetch configuration with catch-all `Request` plus `Image`/`Response` patterns; request-stage settlement through `continueRequest` or `failRequest`; existing fixture Fetch pattern unchanged.

- [ ] **Step 1: Write Fetch-pattern RED tests**

  Assert the controlled fixture still enables exactly one Image/Response pattern. Assert managed mode enables exactly two patterns in order: all-resource Request and Image/Response. No broad response-stage body interception is permitted.

- [ ] **Step 2: Implement the managed Fetch builder**

  Keep `fetch_enable_params()` byte-for-byte behaviorally unchanged for `run`; add `managed_fetch_enable_params()` for `browse`.

- [ ] **Step 3: Write stage-classification and settlement RED tests**

  Use CDP's documented presence rule: either response status or response error means response stage; neither means request stage. Prove block invokes only `failRequest(BlockedByClient)`, unchanged invokes argument-free `continueRequest`, rewrite invokes only URL override, YouTube invokes complete header override, and state changes only after successful CDP settlement.

- [ ] **Step 4: Implement request-stage settlement**

  Load/evaluate policy synchronously in memory, put every CDP call under the five-second acquisition deadline, and treat a settlement failure as fatal. Count blocked and hardened requests only after successful resolution. Do not enter the image response ledger at request stage.

- [ ] **Step 5: Preserve response-stage inference**

  Route response-stage Image pauses through the existing body/decode/inference/replacement path. Ensure a safe image sees one Request continue followed by one Response decision; a domain-blocked image never reaches response inference.

- [ ] **Step 6: Extend privacy-safe summary**

  Add `domain_blocked_requests`, `safe_search_rewrites`, `youtube_restricted_requests`, and `blocklist_entries`. Update JSON key tests and human output. Assert no test URL/domain/query/header value can appear in serialized output.

- [ ] **Step 7: Exercise redirects and cancellation**

  Add orchestration coverage for safe-to-blocked redirect, blocked-to-nowhere termination, navigation cancel during request settlement, and current-vs-obsolete invalid interception. Ambiguous invalid interception remains fatal.

- [ ] **Step 8: Commit locally**

  Run all Rust tests and Clippy; mutate managed Fetch back to image-only and block settlement to continue, require failures, restore, and commit as `Enforce pre-network browser safety layers`.

### Task 4: Reproducible inputs, CLI, and host evidence

**Files:**
- Modify: `flake.nix`
- Modify: `nix/apps.nix` if environment propagation requires it
- Modify: `crates/browser-filter/src/main.rs`
- Modify: `README.md`
- Modify: `docs/experiment-results.md`
- Test: `crates/browser-filter/src/main.rs`, `crates/browser-filter/tests/package_assets.rs`

- [ ] **Step 1: Pin and propagate the list**

  Add the exact `fetchurl` above, export `OMARCHY_KIDS_BLOCKLIST_PATH` through dev shell, checks, package wrapper, and browse app, and include the real-list integration test in both `nix run .#check` and `nix flake check`.

- [ ] **Step 2: Fail startup before Chromium on policy errors**

  Write RED/GREEN CLI seam tests proving missing/unreadable/wrong-count policy input prevents detector or Chromium startup. Load one `DomainPolicy` once and move it into `ManagedBrowser`.

- [ ] **Step 3: Run a harmless live layered smoke**

  Use loopback-only fixtures plus a test policy file containing synthetic reserved domains. Prove blocked requests never hit the HTTP server, safe request/image inference still works, conflicting Google parameters are rewritten, the strict YouTube header reaches a controlled host-routing fixture, and a blocked image still activates page media taint. Do not browse or store explicit content.

- [ ] **Step 4: Measure request-gate overhead**

  Compare at least 1,000 in-memory host decisions and a 100-resource loopback page. Record policy-load time, p50/p95 decision time, and total page overhead. Fail the host gate if p95 policy evaluation exceeds 100 microseconds or if request settlement leaves unresolved pauses.

- [ ] **Step 5: Update host docs honestly**

  Document the layer order, exact pinned source/hash/date/count, offline update model, five-second fail-closed deadline, host-only coverage, video-link rule, remaining bypasses, false positives/negatives, and why this is request interception rather than a system DNS proxy.

- [ ] **Step 6: Verify and commit locally**

  Run `nix run .#check`, `nix flake check`, `nix flake show`, the existing 17-image no-flash fixture, the layered host smoke, profile/process cleanup checks, and `git diff --check`. Commit as `Validate layered host filtering`.

### Task 5: Arch package and installed acceptance contract

**Files:**
- Modify: `packaging/arch/omarchy-kids-browser-filter-demo`
- Modify: `packaging/NOTICES.md`
- Modify: `crates/browser-filter/tests/package_assets.rs`
- Modify sibling: `/home/jake/Projects/omarchy-pkgs/pkgbuilds/omarchy-kids-browser-filter-demo/PKGBUILD`
- Modify: `test/acceptance.d/browser-filter-demo-test.sh`

- [ ] **Step 1: Write package-asset RED tests**

  Require the launcher to export `/usr/share/omarchy-kids-browser-filter-demo/policies/adult-domains.hosts`. Require package metadata to install that regular root-owned 0644 file and its upstream MIT license/notice. Extend the exact package allowlist; unexpected policy/service/autostart files remain forbidden.

- [ ] **Step 2: Update the Arch recipe reproducibly**

  Add the commit-pinned raw list and license to `source` with exact SHA-256 values. Install the list and license under package-owned paths, set the launcher environment, regenerate `.SRCINFO`, and do not run a native build yet.

- [ ] **Step 3: Extend installed acceptance**

  Assert exact policy path, ownership/mode/hash/entry count, summary list count and new aggregate fields, existing model/runtime identity, 17/16/1 fixture behavior, three-sample opaque hold, page media taint, exact 34 privacy-safe model metrics, and complete cleanup. Add a local synthetic domain fixture that proves pre-network blocking without contacting a real adult domain.

- [ ] **Step 4: Run final native Arch preflight**

  In a fresh Arch container, build exactly one package from the exact Kids source snapshot. Validate `.SRCINFO`, all source hashes, exact installed allowlist, ownership/modes, ELF closure, ORT SONAME chain, blocklist hash/count, and real 1x1 inference plus policy load.

- [ ] **Step 5: Commit both repos locally**

  Run all host/package tests and `makepkg --printsrcinfo`. Commit Kids packaging/acceptance as `Package layered browser safety policy`; commit the package recipe as `Include pinned adult-domain policy`.

### Task 6: Fresh unique ISO and VM acceptance

**Files:**
- Modify after successful evidence only: `docs/experiment-results.md`
- Modify after successful evidence only: `plans/managed-browser-content-filter.md`
- Preserve evidence beneath existing local ignored SDD/artifact directories.

- [ ] **Step 1: Freeze and inventory existing evidence**

  Record exact clean heads for Kids, ISO, Omarchy, and packages. Hash all pre-existing ISO/package/base/run artifacts. Verify no QEMU, ports 2222/5905, Kids Chromium, or disposable Kids profile remains except the explicitly user-driven host browser.

- [ ] **Step 2: Run clean readiness**

  Run `nix run .#doctor`, `nix run .#iso-unit`, `nix run .#check`, and `nix flake check` with workspace-path overrides unset. Preserve exact statuses and timings.

- [ ] **Step 3: Build one unique ISO**

  Use `nix run .#kids-iso-build -- --tag layered-kids-<timestamp>` through the reviewed wrapper. Require exactly one immutable tagged ISO and one matching local package archive. Record paths, sizes, modes, and SHA-256 hashes; prove all prior artifacts unchanged.

- [ ] **Step 4: Install into a fresh VM base**

  Require the selected base path to be absent, run install-only without reuse, verify all install phases, package installation, base/OVMF modes/hashes, non-repairing `qemu-img check`, screenshots, and process/listener cleanup.

- [ ] **Step 5: Run normal plus external installed acceptance**

  Reuse the exact base, sync the exact local Omarchy source, and run the Kids external suite. Require normal Omarchy acceptance, no failed units, pinned domain policy, synthetic pre-network denial, strict request hardening, existing image inference/replacement, page-linked video blocking, privacy-safe counts, profile/Chromium cleanup, and clean overlay/base images.

- [ ] **Step 6: Independent whole-range review**

  Trace fail-closed parser/startup, request-stage settlement, redirects, URL/header scope, response-stage inference, media taint, privacy, package allowlist, artifact preservation, and VM cleanup. Fix every Critical/Important finding through a new RED/GREEN cycle and rebuild if installed bytes change.

- [ ] **Step 7: Record the measured result and commit locally**

  Update results and the parent plan with exact evidence and explicit limitations. Run final `nix run .#check`, source-addressed `nix flake check`, diff checks, artifact rehash, repo-cleanliness, and process-cleanup audits. Commit only documentation as `Record layered Kids ISO validation`.

## Completion Claim

Completion means the locally built managed browser and its uniquely tagged ISO demonstrate all seven layers against controlled fixtures: pinned known-domain denial before send, search/video-service restriction, single-tab resource supervision, fail-closed parsing/deadlines, NudeNet image decisions, linked-video suppression after any image replacement, and opaque-cover/placeholder rendering. It does **not** mean perfect pornography detection, arbitrary-video frame classification, protection outside the managed Chromium process, resistance to a privileged local attacker, or redistribution approval.
