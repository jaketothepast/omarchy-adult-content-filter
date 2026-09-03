# Managed-browser host spike final fix report

## Scope and safety

- Worktree: `/home/jake/Projects/omarchy-kids/.worktrees/managed-browser-filter`
- Branch: `managed-browser-filter`
- Requested base: `0e9d6813dc6c4ae9c5fa5a88e92fe1486a221a17`
- Remote state: untouched. No push, pull, PR, fork, remote creation, or remote mutation was performed.
- Commit: recorded in the final handoff because a commit cannot include its own SHA.

## Changes

- Removed the stubbed `doctor` from the flake app surface while retaining the binary subcommand as an explicitly unimplemented placeholder.
- Added valid `meta.description` values to the remaining `infer`, `bench`, `run`, and `check` apps.
- Changed `Policy::decide` to consume already-thresholded detections and replace only the five requested exact exposed-content classes with reason `explicit-detection`.
- Preserved the exact `http://127.0.0.1` fixture-marker override and its `deterministic-fixture` reason as the first decision branch.
- Added policy coverage for all five blocked classes, every other pinned model class plus a near-match, empty reports, fixture precedence, and fixture-origin/marker scoping.
- Corrected README, result, specification, and host-plan claims about runnable apps, stdout/stderr metrics schemas, future telemetry, and unverified Python-reference semantic parity.

## TDD evidence: policy report consumption

The production break named before the test was: ignoring any one of the five already-thresholded exposed classes would incorrectly return `Allow`, while matching broad exposed/covered/anatomical classes would incorrectly expand the narrow spike policy.

### RED

Tests were added before `Policy::decide` was changed.

```text
$ nix develop -c cargo test -p omarchy-kids-browser-filter policy::tests
running 6 tests
test policy::tests::empty_detection_report_is_allowed ... ok
test policy::tests::covered_ambiguous_and_near_match_classes_are_allowed ... ok
test policy::tests::ordinary_urls_are_allowed_after_inference ... ok
test policy::tests::flagged_fixture_url_replaces_only_after_receiving_an_inference_report ... ok
test policy::tests::marked_non_fixture_urls_are_allowed_after_inference ... ok
test policy::tests::exact_explicit_detection_classes_are_replaced ... FAILED

thread 'policy::tests::exact_explicit_detection_classes_are_replaced' panicked at crates/browser-filter/src/policy.rs:124:13:
assertion `left == right` failed: expected BUTTOCKS_EXPOSED to replace
  left: Allow
 right: Replace { reason: "explicit-detection" }

test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 52 filtered out
```

The failure was the expected missing behavior, not a compile error or fixture problem.

### GREEN

After adding the minimal exact-class match after the deterministic fixture branch:

```text
$ nix develop -c cargo test -p omarchy-kids-browser-filter policy::tests
running 6 tests
test policy::tests::covered_ambiguous_and_near_match_classes_are_allowed ... ok
test policy::tests::exact_explicit_detection_classes_are_replaced ... ok
test policy::tests::empty_detection_report_is_allowed ... ok
test policy::tests::ordinary_urls_are_allowed_after_inference ... ok
test policy::tests::flagged_fixture_url_replaces_only_after_receiving_an_inference_report ... ok
test policy::tests::marked_non_fixture_urls_are_allowed_after_inference ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 52 filtered out
```

The exact blocked set exercised by the test is `BUTTOCKS_EXPOSED`, `FEMALE_BREAST_EXPOSED`, `FEMALE_GENITALIA_EXPOSED`, `ANUS_EXPOSED`, and `MALE_GENITALIA_EXPOSED`. The allow set covers the other 13 pinned labels and `BUTTOCKS_EXPOSED_EXTRA`, proving exact rather than prefix or broad “exposed” matching.

## CLI and flake app surface

The surface check asserted binary help, the reserved command's failing behavior, exact evaluated app names, and nonempty evaluated descriptions:

```text
$ <CLI/app surface assertion script>
CLI help: doctor placeholder plus infer/bench/run
doctor exit: 1, explicit unimplemented error
flake apps: ["bench","check","infer","run"], all descriptions nonempty
```

`nix flake show --json` evaluated this app object:

```json
{
  "bench": { "description": "Benchmark the pinned local ONNX image detector", "type": "app" },
  "check": { "description": "Run formatting, lint, and workspace tests", "type": "app" },
  "infer": { "description": "Run bounded local ONNX inference for one image", "type": "app" },
  "run": { "description": "Run the controlled headed Chromium interception experiment", "type": "app" }
}
```

There is no evaluated `doctor` app. The only warnings during this pre-commit evaluation were Nix's dirty-Git-tree warnings; there were no missing-app-metadata warnings.

## Automated verification

### Formatting

```text
$ cargo fmt --check
<exit 0; no output>
```

### Full workspace and real-model integration

```text
$ nix develop -c cargo test --workspace
library: 58 passed; 0 failed
binary: 4 passed; 0 failed
infer CLI integration: 1 passed; 0 failed
doc tests: 0 failed
```

The visible run included the real-model `infer_emits_one_safe_json_record_for_a_generated_png` integration, model-content hash rejection, runtime-reported identity, colored-pixel RGB/RGBA-to-BGR preprocessing regressions, output decoding/NMS tests, the six policy tests, browser lifecycle tests, and the exact metrics allowlist test.

### Full flake check

```text
$ nix flake check
checking app 'apps.x86_64-linux.check'...
checking app 'apps.x86_64-linux.infer'...
checking app 'apps.x86_64-linux.bench'...
checking app 'apps.x86_64-linux.run'...
running 1 flake checks...
all checks passed!
```

The pre-commit run had only the expected dirty-tree warning. A clean-tree post-commit rerun is recorded in the final handoff to prove warning-free output.

### Exact headed browser proof

```text
$ nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
```

The command emitted 34 per-image stage records and one summary. Every stage record used only `stage`, `verdict`, `fixture_index`, and `elapsed_micros`; fixture 5's inference and policy records had verdict `replace`, while every other fixture had verdict `allow`.

```json
{
  "chromium_version": "Chrome/152.0.7977.75",
  "intercepted": 17,
  "continued": 16,
  "replaced": 1,
  "unresolved": 0,
  "clean_shutdown": true,
  "reveal_latency_millis": 4,
  "no_flash_assertion": {
    "requested_hold_millis": 500,
    "actual_hold_millis": 500,
    "hold_screenshot_count": 3,
    "hold_sampled_pixels": 1413000,
    "reveal_screenshot_count": 1,
    "reveal_sampled_pixels": 471000,
    "cover_rgba": [17, 19, 24, 255],
    "safe_fixture_colors_present": 16,
    "placeholder_rgba": [255, 0, 255, 255],
    "placeholder_color_present": true,
    "original_flagged_rgba": [5, 0, 0, 255],
    "original_flagged_color_absent": true
  }
}
```

This confirms 17 intercepted, 16 continued, 1 replaced, 0 unresolved, no flash during the hold, correct post-reveal pixels, and clean Chromium shutdown. The fixture marker—not model accuracy—deterministically caused the live replacement.

## Documentation and placeholder review

The current contracts documented by the final tree are:

- `run` per-stage/per-image privacy-safe `MetricRecord` JSONL goes to stderr with only `stage`, `verdict`, `fixture_index`, and `elapsed_micros`.
- `run --json` writes one headed summary JSON object to stdout; without `--json`, stdout receives the human count summary.
- `bench --json` writes four workload-summary JSONL records to stdout with CPU/model/runtime/build/workload metadata, encoded-byte median, and decode/preprocess/inference/postprocess/total p50/p90/p95 objects.
- Rich run IDs, cache outcomes, response-byte fields, pause-to-fulfill timing, cover-duration breakdowns, and memory measurements are explicitly future fields.
- Per-image metrics have no content or URL fields; other outputs have no raw image bytes or URLs, while the headed summary accurately documents its controlled-fixture DOM RGBA metadata.
- Python-reference semantic parity is explicitly unverified. No checked-in golden exists; real model checksum/runtime execution and colored-pixel preprocessing evidence are preserved without promoting them to parity evidence.
- The host milestone does not claim that a runnable doctor exists. The separate ISO workflow plan remains the owner of a future real doctor.

The progress ledger's placeholder-scan ruling excludes the host plan because that plan contains the literal placeholder regex in its own verification command. The three published deliverables were scanned for placeholders, while the host plan remained included in the separate stale-claim review:

```text
$ <published placeholder and four-finding stale-claim assertion script>
crates/browser-filter/src/policy.rs:26:        } else if report.detections.iter().any(|detection| {
published placeholder scan: clean
four-finding stale-claim scan: clean
git diff --check: clean
```

The negative assertions covered stale runnable-doctor commands/app sets, the ignored-report implementation shape, the old rich/stdout-only metrics contract, and the old completed-reference-golden claim. Positive assertions confirmed report consumption and explicit unverified-parity language.

## Files changed

- `README.md`
- `crates/browser-filter/src/policy.rs`
- `docs/experiment-results.md`
- `flake.nix`
- `plans/2026-09-03-managed-browser-host-spike-implementation.md`
- `plans/managed-browser-content-filter.md`
- `.superpowers/sdd/2026-09-03-managed-browser-host-spike-implementation/final-fix-report.md`

## Remaining concerns

- The five-class policy consumes already-thresholded model results but does not establish classifier accuracy, calibration, false-positive/false-negative behavior, or a production policy.
- The harmless live fixture validates deterministic replacement plumbing; it does not exercise a real explicit model detection.
- Python-reference semantic parity remains unverified by design in this fix wave.
- The host flake intentionally has no runnable doctor app until the separate ISO workflow implements one.
- Rich browser overhead, cache, cover-duration, run-identity, and memory telemetry remains unimplemented and is documented as future work.
