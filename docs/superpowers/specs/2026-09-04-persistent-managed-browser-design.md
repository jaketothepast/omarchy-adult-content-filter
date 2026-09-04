# Persistent Managed Browser Design

## Purpose

Add a host-side manual browsing mode to the existing Omarchy Kids experiment. The mode keeps one headed Chromium tab open so a person can drive it, intercepts ordinary HTTP(S) image responses, runs the pinned local NudeNet/ONNX detector, replaces explicitly detected or unclassifiable images, and exits cleanly when Chromium is closed or the controller receives Ctrl-C.

This is a driveable engineering prototype, not a claim that the browser is safe for unsupervised children. The first manual mode intentionally has a narrow, inspectable surface and retains the existing controlled fixture experiment unchanged.

## User Interface

The new command is:

```bash
nix run .#browse
nix run .#browse -- --url https://example.com --json
```

Without `--url`, Chromium opens a supervised `about:blank` tab and the user types a destination. The command stays attached for the lifetime of the browser. Closing the primary tab/browser or pressing Ctrl-C initiates bounded Chromium and inference-worker cleanup. A final summary is written only after cleanup; `--json` selects machine-readable output.

The current `nix run .#run` fixture command and all of its validation remain byte-for-byte behaviorally unchanged.

## Safety Boundary

- Chromium always uses a new disposable profile, never the user's default profile.
- The existing document-start extension covers new documents before page content is painted. The controller removes the top-level cover only after the page load event is received and no intercepted image response remains unresolved.
- Only one page target is supervised in this first version. Any additional `page` target is closed. A failure to close it is fatal and closes the entire managed browser.
- Fetch interception remains at the CDP `Image`/`Response` stage with Chromium cache disabled.
- A response is allowed only when it is an HTTP redirect with no rendered body or when its body is acquired, bounded, decoded, inferred, and receives `Policy::Allow`.
- Response errors, missing/unexpected status, non-200 terminal statuses, oversized bodies, unsupported formats, decode errors, model errors, worker errors, and the five-second acquisition or inference deadline all produce the existing replacement PNG. They do not terminate browsing if replacement succeeds.
- Failure to issue the resolving CDP `continueResponse` or `fulfillRequest` command is fatal. The browser is closed so an unresolved response cannot escape supervision.
- The inference worker remains serial and bounded. Backpressure is fail closed at the same five-second inference deadline.
- Per-page load that never settles remains covered. The Chrome toolbar remains usable so the user can navigate away or close the browser.
- Metrics and the final summary contain counts, timings, runtime identity, and lifecycle status only. They contain no URL, response body, image bytes, page title, filesystem path, or browsing history.

## Architecture

`crates/browser-filter/src/managed.rs` owns the manual-mode configuration, privacy-safe summary, response decision, single-page CDP loop, additional-target closure, and top-level reveal state. It reuses the proven detector worker, Fetch request builders, response-body decoder, replacement bytes, and exhaustive Chromium cleanup from `browser.rs` through crate-private interfaces.

The controller launches Chromium and creates one `about:blank` page. It registers Fetch, load, and browser target listeners before optional initial navigation. One `tokio::select!` loop serially handles image responses, top-level load events, additional target creation, stream termination, and Ctrl-C. Serial handling deliberately matches the current single-detector pipeline and avoids out-of-order response resolution.

For each image pause, a small pure status planner chooses redirect continuation, body classification, or immediate fail-closed replacement. Classification produces one of `allow`, `explicit_replace`, or `failed_closed`. The CDP response is resolved exactly once before counters change. A pause ledger makes duplicate or unresolved request IDs fatal.

## Timeouts

- Body acquisition and each CDP response-resolution call: 5 seconds.
- Queue admission plus local inference result: 5 seconds total.
- Browser launch and cleanup stages: 30 seconds.
- There is no total session deadline; the user owns session lifetime.
- A page without a load event is never automatically revealed.

## Single-Tab Rule

Browser-level `Target.targetCreated` events are monitored. The one primary page target is retained; every later target whose type is `page` is closed under the five-second acquisition deadline and counted. Non-page internal targets are ignored. Existing pages present at startup are closed after the primary page exists. If target inspection or closure fails, the managed session fails closed by closing Chromium.

## Error Handling and Cleanup

Classification failures are expected fail-closed outcomes and increment `failed_closed`; they are not session errors. CDP transport failure, listener termination while Chromium remains active, target-control failure, duplicate request IDs, or replacement failure ends the session. Regardless of result, the controller shuts down the detector worker, closes/kills/waits for Chromium with the existing exhaustive cleanup, flushes no browsing-sensitive data, and removes the disposable profile. Multiple errors are aggregated.

## Verification

Unit tests cover configuration validation, every response-status plan, fail-closed error conversion, policy outcomes, exact once-only pause settlement, load/unresolved reveal gating, privacy-safe summary schema, single-tab target decisions, and CLI parsing.

A real headed host smoke uses a loopback page with a valid harmless PNG and a deliberately undecodable image response. It proves the harmless image is continued, the undecodable image is replaced, the page reveals only after both resolve, Ctrl-C returns a clean summary, the profile disappears, and Chromium exits. The final user-driven launch starts from `about:blank` and remains running for the user.

## Explicit Limitations

This iteration does not cover video, audio, canvas, WebGL, CSS background images, `data:` URLs, `blob:` URLs, browser chrome, downloads, DevTools, service-worker/cache variants, additional tabs/windows, extension tampering, process tampering, or adversarial pages. Subframes remain covered because readiness is applied only to the supervised top-level document. These limits mean the mode must not be represented as production child safety or arbitrary-site pornography blocking.
