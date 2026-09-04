# Managed-browser host experiment results

## Result

On 2026-09-03, all three reproduction commands completed successfully on the machine described below. The controlled headed-browser run intercepted 17 harmless PNG responses, continued 16, replaced the deterministically marked fixture at index 5, revealed only after every response was resolved, shut Chromium down cleanly, and removed its disposable profile.

This result proves the controlled response-interception, cover, and replacement plumbing. It does not validate NudeNet accuracy or real-world pornography blocking. The flagged route is a deterministic policy fixture, not an accuracy test: the model runs for timing, then the fixture marker overrides the final verdict so the replacement branch always executes. No Python-reference golden was produced, so semantic parity with the upstream reference implementation remains unverified.

## Reproduction commands

Run from the repository root:

```bash
nix flake check
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
```

These are the exact commands used for this result. `nix flake check` finished with `all checks passed!`. `bench --json` emitted four workload-summary JSONL records to stdout. `run --json` emitted one headed-experiment summary JSON object to stdout and two privacy-safe per-image stage records to stderr for each intercepted fixture. Each stderr `MetricRecord` contains exactly `stage`, `verdict`, `fixture_index`, and `elapsed_micros`; it contains no image content or URL. The measurements below are transcribed from that fresh output.

The benchmark records contain `cpu_model`, `onnx_runtime_version`, `model_sha256`, `build_mode`, `image_count`, `warmups`, `iterations`, `encoded_bytes_median`, and p50/p90/p95 objects for decode, preprocessing, inference, postprocessing, and total workload time. The current headed summary contains Chromium/version and lifecycle, reveal, DOM-pixel, and optional no-flash assertion results. Rich run identifiers, cache outcomes, pause-to-fulfill timing, cover-duration breakdowns, and memory measurements remain future telemetry; they are not emitted by this implementation.

## Environment

| Item | Measured value |
| --- | --- |
| Source revision tested | `74f43a138f39af4a650883f1b8d0a0abe81073a6` (`Keep experiment workflow local only`) |
| Measurement time | `2026-09-03T19:08:14-04:00` |
| Operating system | Arch Linux rolling |
| Kernel | Linux `7.1.4-arch1-1`, x86_64 |
| CPU | AMD Ryzen 5 7600 6-Core Processor; 1 socket, 6 cores, 2 threads per core, 12 logical CPUs |
| Memory reported by `free -b` | 66,578,616,320 bytes total |
| Nix | 2.34.8 |
| Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, x86_64-unknown-linux-gnu, LLVM 21.1.8 |
| Chromium | `152.0.7977.75` (`Chromium 152.0.7977.75` from the Nix environment; `Chrome/152.0.7977.75` from CDP) |
| ONNX Runtime | 1.27.1 (`libonnxruntime.so.1.27.1`) |
| Model | NudeNet `320n.onnx` from upstream commit `6ccc81c6c305cccfd46d92b414f8a5c0a816574d` |
| Verified model SHA-256 | `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f` |
| Benchmark build | release |

The model checksum was independently read with `sha256sum "$NUDENET_MODEL_PATH"` inside `nix develop`; it matches every benchmark record.

## Release benchmark

Each workload used deterministic, harmless, in-memory 1280×720 JPEGs, three complete warmup passes, and 20 measured iterations. The synthetic color formula has a 16-image period: the 19- and 62-image workloads reuse generated patterns after index 15. The 62-image workload is therefore a throughput/load exercise, not a 62-image diversity corpus.

Stage percentiles are taken across all per-image samples in a workload (`image_count × 20`). Total-workload percentiles are taken across the 20 complete workload iterations, so a total percentile is not the sum of the stage percentiles. All timing values below are exact integer microseconds from the JSON output.

| Images | Dimensions | Median encoded bytes per image | Warmup passes | Measured iterations |
| ---: | --- | ---: | ---: | ---: |
| 1 | 1280×720 | 39,751 | 3 | 20 |
| 13 | 1280×720 | 39,914 | 3 | 20 |
| 19 | 1280×720 | 39,946 | 3 | 20 |
| 62 | 1280×720 | 39,946 | 3 | 20 |

| Images | Stage | p50 (µs) | p90 (µs) | p95 (µs) |
| ---: | --- | ---: | ---: | ---: |
| 1 | Decode | 1,590 | 1,707 | 1,784 |
| 1 | Preprocess | 10,690 | 11,325 | 11,398 |
| 1 | ONNX inference | 4,528 | 4,980 | 5,098 |
| 1 | Postprocess | 36 | 38 | 39 |
| 1 | Total workload | 17,117 | 17,613 | 17,934 |
| 13 | Decode | 1,603 | 1,703 | 1,758 |
| 13 | Preprocess | 10,615 | 11,347 | 11,488 |
| 13 | ONNX inference | 4,479 | 6,105 | 6,669 |
| 13 | Postprocess | 40 | 41 | 43 |
| 13 | Total workload | 223,027 | 228,293 | 233,792 |
| 19 | Decode | 1,610 | 1,739 | 1,811 |
| 19 | Preprocess | 10,709 | 11,484 | 11,634 |
| 19 | ONNX inference | 4,549 | 6,929 | 7,404 |
| 19 | Postprocess | 39 | 41 | 43 |
| 19 | Total workload | 333,641 | 342,349 | 346,725 |
| 62 | Decode | 1,603 | 1,698 | 1,795 |
| 62 | Preprocess | 10,650 | 11,401 | 11,545 |
| 62 | ONNX inference | 4,490 | 5,889 | 7,299 |
| 62 | Postprocess | 40 | 41 | 42 |
| 62 | Total workload | 1,073,885 | 1,090,104 | 1,092,141 |

## Headed-browser proof

Chromium reported these final counts and lifecycle values:

| Measurement | Result |
| --- | ---: |
| Intercepted image responses | 17 |
| Continued original responses | 16 |
| Replaced responses | 1 |
| Unresolved response pauses | 0 |
| Reveal latency after the last response settled | 2 ms |
| Clean Chromium shutdown | true |
| Disposable profile removed | true; the command checks removal before writing its successful summary |

The per-fixture `inference` records below measure the complete detector path (decode + preprocess + ONNX inference + postprocess), not only the ONNX call. Policy timing measures only `Policy::decide`.

| Fixture index | Final verdict | Detector path (µs) | Policy (µs) |
| ---: | --- | ---: | ---: |
| 0 | allow | 34,792 | 0 |
| 1 | allow | 32,012 | 0 |
| 2 | allow | 14,764 | 0 |
| 3 | allow | 13,249 | 0 |
| 4 | allow | 12,280 | 0 |
| 5 | replace | 11,487 | 1 |
| 6 | allow | 11,500 | 0 |
| 7 | allow | 11,651 | 0 |
| 8 | allow | 7,760 | 0 |
| 9 | allow | 6,078 | 0 |
| 10 | allow | 7,902 | 0 |
| 11 | allow | 6,076 | 0 |
| 12 | allow | 7,458 | 0 |
| 13 | allow | 8,494 | 0 |
| 14 | allow | 6,881 | 0 |
| 15 | allow | 6,479 | 0 |
| 16 | allow | 6,248 | 0 |
| **Total** | **16 allow / 1 replace** | **205,111** | **1** |

### Cover and pixel assertions

The browser deliberately held the flagged response for 500 ms. The actual hold was 501 ms. During that interval, three screenshots sampled 1,413,000 pixels; every sampled pixel was the opaque cover color `[17, 19, 24, 255]`.

After response settlement and reveal, one screenshot sampled 471,000 pixels and established all of the following:

- all 16 safe fixture colors were present;
- the local placeholder color `[255, 0, 255, 255]` was present; and
- the original flagged fixture color `[5, 0, 0, 255]` was absent.

The DOM inspection also reported all 17 images at their expected 1×1 natural size and expected RGBA value, including the placeholder at index 5. These pixel checks demonstrate the controlled sequence only; they do not establish flash-free behavior across every Chromium cache, navigation, or rendering path.

## Performance-gate assessment

| Experiment gate | Evidence | Assessment |
| --- | --- | --- |
| Median single-image ONNX inference ≤25 ms | 1-image p50 was 4.528 ms | Pass on this machine |
| 17-image total inference ≤350 ms | The headed run's 17 complete detector-path records totaled 205.111 ms, which is a stricter duration than ONNX calls alone | Pass for this run |
| Median pause-to-fulfill overhead excluding inference ≤10 ms/image | Current JSON isolates detector and policy time but does not measure body acquisition plus CDP continue/fulfill overhead | Not evaluated |
| Reveal ≤500 ms after the last required response | 2 ms | Pass for this run |
| 62-image workload ≤1.5 s without unbounded memory growth | Total workload was 1.073885 s p50, 1.090104 s p90, and 1.092141 s p95; the command emitted no memory measurement | Timing passes; memory condition not evaluated |

These results are machine-specific experiment measurements, not latency guarantees for other Omarchy hardware.

## Baseline local-source ISO validation

On 2026-09-04, the Nix-supplied sibling workflow built a local Omarchy ISO, installed it through the real graphical configurator in QEMU/KVM, and passed the complete shortcut and in-guest acceptance smoke against a reusable installed-system base. This is an upstream Omarchy baseline only. It does not contain the Omarchy Kids browser-filter package, managed Chromium policy, filter service, or Kids-specific acceptance test.

The ISO contains the local Omarchy compatibility branch through `5df95727`, including the generic Broadcom package-name correction and bar probe fixes discovered during validation. The builder and host harness likewise contain local generic fixes listed below. No Kids filtering code was copied into the ISO.

### Exact source identities

| Checkout | Branch | Revision used for the final artifact or validation |
| --- | --- | --- |
| Omarchy Kids wrapper | `managed-browser-filter` | `42ac5f56fb40907e5bde1aa03fc026d6cd42b417` |
| Omarchy source packaged in the ISO | `omarchy-kids-iso-compat` | `5df95727eaaa072087cbe63d81eb2789ee5f0eec` |
| Omarchy ISO builder used for the final build | `omarchy-kids-local-workflow` | `769199619c87972258bb9513498b5b08013a7041` |
| Omarchy ISO harness used for the final acceptance pass | `omarchy-kids-local-workflow` | `06a3718b16f70514eca87cd8c2e1bf74bd79e369` |
| Omarchy ISO integration parity fix verified afterward | `omarchy-kids-local-workflow` | `55e47941d6d4b30eb35b32f5806c70f6110dea2f` |
| Omarchy packages | `master` | `18f11555b690ff5dd8a1b7b4371adfce9963ffa2` |
| ArchISO submodule | detached | `424e78130db2af6c1ceb55b442d7914b1109ff2b` |

The final build log records `omarchy-dev-4.0.0.r2018.g5df9572-1-any.pkg.tar.zst`, independently tying the packaged Omarchy source to the revision above. All repositories were clean after their local commits. None of these local branches or commits was pushed.

### Exact successful commands

Each command was run from this linked Kids worktree after unsetting `OMARCHY_PATH`, `OMARCHY_ISO_PATH`, and `OMARCHY_PKGS_PATH`, so the checked sibling defaults were exercised rather than the installed Omarchy snapshot in the interactive shell.

```bash
nix run .#doctor
nix run .#iso-unit
nix run .#iso-build
nix run .#iso-test -- /home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-local.iso --install-only --no-preview
nix run .#iso-test -- /home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-local.iso --reuse-base --sync-omarchy /home/jake/Projects/omarchy --no-preview
```

All five final commands exited `0`. Doctor passed all 13 real host checks. The VM-free ISO suite passed every shell case and all 63 Python tests. The final build took 270 seconds, the final install-only run took 476 seconds, and the post-fix reuse-base acceptance pass took 155.310 seconds. The in-guest acceptance portion itself reported 85 seconds.

### Artifact identity

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-local.iso` | 6,191,368,192 bytes | `3a5dd741741ed2f01dd84b3509618c781bce2998463b118bb064bc22b7fc9cc9` |
| `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-local/base.qcow2` | 6,360,268,800 bytes | `c7d20e572264adf71abc7b3c8c81b81a4e9b180dc77c013940c20a0744ccc77a` |
| `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-local/OVMF_VARS.4m.fd` | 540,672 bytes | `f175d0dcd5ce7c9765b8cfcf0200002b97e2359b4c3cbf1aaab7a739911dc4f5` |

The ISO is the single final `*-local.iso`; two superseded images were retained rather than deleted: `omarchy-2026.09.04-x86_64-local-before-bar-reprobe.iso.preserved` (6,191,368,192 bytes, SHA-256 `29ec38e317460faec85a23f8ce6f517dce05ba140a8e97e9019de61689263726`) and `omarchy-2026.09.04-x86_64-local-attempt-8-bar-reprobe.iso.preserved` (6,191,368,192 bytes, SHA-256 `811b732436451e27d293a12279449d81deb3f8fa44d2c330cc3b7d0d06903f60`). A fresh non-repairing `qemu-img check` found no errors in the final base.

The final install artifacts are under `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-local/runs/20260903-231936`. The canonical passing acceptance artifacts, including 49 screenshots and the collected guest logs, are under `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-local/runs/20260904-000715`. Command logs, timings, controlled-boot evidence, extracted UKI, Limine configuration, Plymouth units, and read-only journal evidence are preserved under `.superpowers/sdd/2026-09-03-nix-iso-workflow-implementation/task-5-artifacts/`.

### Attempt history and defects found

No failed command was relabeled as a pass, and no assertion was weakened.

| Stage | Attempt | Wall time | Exit | Outcome |
| --- | ---: | ---: | ---: | --- |
| Build | 1 | 24.515s | 1 | Pacman rejected a stale host-cached `omarchy-keyring` archive. |
| Build | 2 | 15.511s | 1 | Invocation from the Kids worktree ran ISO-relative submodule setup in the wrong directory, leaving the ArchISO releng profile unavailable. |
| Build | 3 | 26.102s | 1 | An inherited `OMARCHY_PATH` selected an older installed snapshot that lacked the current settings file. The clean measurement shell now exercises default sibling resolution without changing override precedence. |
| Build | 4 | 95.760s | 1 | Current edge repositories no longer provided `broadcom-wl`; both the offline inventory and hardware installer were corrected to the supported `broadcom-wl-dkms` package. |
| Build | 5 | 242.668s | 1 | Pacman rejected a stale host-cached `tzupdate` archive. |
| Build | 6 | about 178s | 1 | A rebuilt local `omarchy-settings-dev` archive collided with same-version bytes in the persistent host Pacman cache. The captured timer line was malformed, so no false precision is claimed. Local-source builds now keep an invocation-local package cache while retaining the channel-scoped offline mirror. |
| Build | 7 | 273.09s | 0 | First complete ISO build; retained as the pre-bar artifact and superseded after acceptance exposed generic Omarchy issues. |
| Build | 8 | 280s | 0 | Rebuilt after the first bar replay fix; retained and superseded after review required behavioral state-machine coverage. |
| Build | 9 | 270s | 0 | Final ISO, packaged from Omarchy `5df95727`. |
| Install | 1 | 1.75s | 1 | QEMU could not write the Nix-store-derived OVMF vars copy because plain copying preserved mode `0444`; all mutable-copy paths now create writable destinations. |
| Install | 2 | 336.06s | 1 | The harness waited for the obsolete greeter marker `Opinionated`; the real ISO displayed the current distinctive marker `Agentic`. |
| Install | 3 | 475.92s | 0 | Installed successfully and produced an intermediate base; it was later superseded after rebuilding the ISO. One bounded SSH bootstrap attempt timed out before the retry succeeded. |
| Install | final | 476s | 0 | Final attempt-9 ISO installed and produced run `20260903-231936` plus the reusable base above. One bounded SSH bootstrap attempt timed out before the retry succeeded. |
| Acceptance | 1 | 155.34s | 1 | The visible reminder prompt was missed by Tesseract sparse-text mode. A same-image mode-6 fallback was added only after the primary mode-11 miss. |
| Acceptance | command retry | 0s | 1 | The wrapper was accidentally invoked from the ISO checkout, which is not a flake; this operator error was preserved and the command was rerun from the Kids worktree. |
| Acceptance | 3 | 171s | 1 | A rapid hide/reveal sequence lost a bar position probe. The production request path now coalesces one pending replay, with the async transition behavior tested independently. |
| Acceptance | pre-mitigation final | 1,360s | 1 | The configured 600-second SSH wait consumed roughly twice that wall time, then continued because readiness failure was not propagated. The guest eventually booted but correctly failed the `no failed system units` assertion on `plymouth-start.service`. |
| Acceptance | controlled boot | 62.805s | 0 | An isolated overlay with identical base/firmware/device arguments and `-serial none` received no guest input or screenshot for 60 seconds, then passed the first SSH probe. Only `tty0` was active, no `console=` was injected, both Plymouth units succeeded, no unit failed, and shutdown plus `qemu-img check` were clean. |
| Acceptance | canonical final | 155.310s | 0 | Run `20260904-000715` passed every shortcut smoke and all in-guest suites, including bar hide/park/reveal, reminder OCR, the complete package manifest, and no failed system or user units. |

Read-only diagnosis of the 1,360-second failure found `initramfs_async=0` in the actual installed UKI and Limine entry, clean base and overlay images, and a Plymouth readiness stall immediately after ANSI terminal queries on QEMU's output-only `file:` serial backend. The successful first boot completed the same Plymouth phases in milliseconds. The narrow host-harness fix removes that artificial inputless serial console from headless acceptance and integration VMs, uses an absolute wall-clock SSH deadline, and propagates readiness failure. It does not change the installed kernel command line, Plymouth units, or failed-unit assertion.

Behavioral RED/GREEN coverage was added for each generic fix: working-directory anchoring and caller-relative source preservation; published-versus-local package-cache mounts; writable OVMF copies; current greeter text; ordered OCR fallback; bar replay transitions; exact QEMU serial arguments; acceptance wall deadlines and propagation; and integration deadline/bootstrap/factory-reset propagation. The final ISO repository suite passes all shell tests and 63 Python tests.

One unrelated host-side Omarchy aggregate result remains recorded: `./test/all` reported 2 of 227 shell test files failing, `launch-about-test.sh` at `a roomy window animates` and `network-captive-portal-test.sh` because `quickshell` was unavailable in that host test environment. The focused OCR and bar state-machine regressions passed, and the final installed guest acceptance suite passed the corresponding runtime surfaces. These aggregate failures were not changed or concealed as part of the ISO baseline.

## Final installed Omarchy Adult Content Filter validation

On 2026-09-04, the renamed browser-only product was built as the private Arch package `omarchy-adult-content-filter`, included in a uniquely tagged Omarchy ISO, installed through the real graphical configurator, and exercised in a headed QEMU/KVM Omarchy session. The user-facing shape is an opt-in Omarchy application/plugin; the Arch package is its delivery mechanism. It installs no service, autostart entry, MIME association, default-browser override, global Chromium policy, account restriction, or sudo rule.

The package bytes came from filter revision `eb74d00872166d5681d48bda5da6688eb310af4e`, package recipe `6ee53e61bfb3ef57ec66db328e9b3dc03eacb48b`, generic ISO workflow `29a66a248bf21079f479eccfe21067331d615079`, and Omarchy `fb39bcd3b92cd70eebcdaf31945b91260f2a0f94`. Installed acceptance used filter revision `9d2932970d2d8c9e05831dff7e07cb91b38adc80`, whose only post-build change makes the harness recognize Chromium's rewritten process title and use Hyprland 0.56's Lua window dispatcher. The package and ISO bytes were not rebuilt or altered by that harness-only compatibility fix.

| Artifact | Size | Mode | SHA-256 |
| --- | ---: | ---: | --- |
| `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-adult-filter-final-20260904-174935-254772802.iso` | 6,209,560,576 bytes | `0644` | `c3bb3149668e1e054c5942a342e12201cd4e1eacc753664f9566fdaddb2870d2` |
| `/home/jake/Projects/omarchy-iso/release/local-packages/adult-filter-final-20260904-174935-254772802/omarchy-adult-content-filter-0.1.0-1-x86_64.pkg.tar.zst` | 23,781,775 bytes | `0644` | `8c6324c3b880af6d87a06cf4abaaaee48c2a0ac70001f4b82fe0515aebb93e2b` |
| Installed 40 GiB `base.qcow2` | 6,485,835,776 bytes | `0644` | `28989d415d91bf1374ddd856c6ba3a7f569bee320a3dd19227233a854510b65c` |
| Passing acceptance `run.qcow2` overlay | 148,111,360 bytes | `0644` | `bec7f0538d205f04258722f626d640e45221ec52a77898a6a087ab8d242d3c3a` |

The ISO produced exactly one matching package archive. `pacman -Qip` identifies it as private-evaluation package `omarchy-adult-content-filter` version `0.1.0-1`; its 29-entry `pacman -Qlp` inventory contains the dedicated command and desktop entry, private Rust supervisor, exact two-file cover extension, pinned model and 76,767-entry adult-domain policy, private ONNX Runtime, and their notices. The real installer completed all 14 recorded phases, the second bounded SSH bootstrap attempt succeeded, and the installed-system pacman log records `omarchy-adult-content-filter (0.1.0-1)`. Install evidence is retained under run `20260904-135712`.

The final reuse-base run `20260904-143640` passed all 26 desktop smoke checks and normal Omarchy acceptance in 86 seconds, including no failed system or user units. The external suite then observed the exact newly launched Chromium client and PID, verified `--disable-dev-tools`, native Wayland, and the private disposable profile, closed that addressed window through the current Hyprland dispatcher, and saw the launcher exit cleanly. The managed summary reported Chromium `152.0.7977.82`, ONNX Runtime `1.27.1`, the pinned model hash, 76,767 loaded blocklist entries, one rejected extra page, zero unresolved requests, and clean shutdown.

The same installed binary's harmless controlled fixture independently reported 17 intercepted images, 16 continued, one replaced, zero unresolved, and 2 ms reveal latency. Its requested 1,500 ms cover lasted 1,501 ms across three samples totaling 1,413,000 opaque `[17, 19, 24, 255]` pixels. After reveal, all 16 safe colors and the magenta placeholder were present while the original flagged color was absent. The metric file contained exactly 34 objects with only `elapsed_micros`, `fixture_index`, `stage`, and `verdict`: 17 inference, 17 policy, 32 allow, and two replace records. Both base and overlay passed non-repairing `qemu-img check`; the ISO, package, and base hashes remained unchanged; no disposable profile, filter-launched Chromium, QEMU process, or ports 2222/5905 remained.

Host tests separately exercise the layered request policy: pre-network adult-domain denial, Google SafeSearch rewriting, strict YouTube restriction headers, response-stage JPEG/PNG inference with fail-closed replacement, and blocking only media controls and video elements connected to a flagged image. The installed harmless fixture proves packaging, execution, cover, image substitution, and cleanup, but it does not claim pornography-classifier accuracy or exercise live adult sites. This browser-only plugin also does not stop an administrator or ordinary user from launching a different browser; account and OS enforcement remain deliberately outside this deliverable.

All artifacts and commits remain local. No push, fetch, fork, pull request, remote mutation, upload, or evidence deletion was performed.

## Superseded private installed Kids ISO validation

On 2026-09-04, the reviewed post-fix source heads produced a fresh private Kids package, a fresh uniquely tagged ISO, a fresh installed-system base, and a canonical normal-plus-external acceptance run. This is the final approved controlled-demo artifact. The first successful installed artifact remains documented in the next section as superseded-but-successful evidence; it was not deleted, renamed, or overwritten.

The final result retains the same narrow claim boundary. It proves exact Arch packaging, offline inclusion and installation, the dedicated controlled launcher, the harmless 17-image fixture's interception and opaque cover, one deterministic replacement, the four-key privacy-safe metric stream, pinned detector/runtime identity, and cleanup in an installed Omarchy VM. It does not prove arbitrary-site browsing, pornography-classifier accuracy, adversarial or bypass resistance, tamper resistance, service supervision, safe default-browser or managed-browser policy, or redistribution rights.

### Reviewed source identities

Every repository was clean and had no upstream tracking before the build. The commands ran with `OMARCHY_PATH`, `OMARCHY_ISO_PATH`, and `OMARCHY_PKGS_PATH` unset so the reviewed sibling checkouts resolved directly.

| Checkout | Branch | Reviewed revision used |
| --- | --- | --- |
| Omarchy Kids package, wrappers, and external acceptance | `managed-browser-filter` | `d2f0a2c9dc67486514285ed3203bd7171044c64b` |
| Omarchy ISO generic package/build/acceptance interfaces | `omarchy-kids-local-workflow` | `29a66a248bf21079f479eccfe21067331d615079` |
| Omarchy source packaged into the ISO and synced for normal acceptance | `omarchy-kids-iso-compat` | `fb39bcd3b92cd70eebcdaf31945b91260f2a0f94` |
| Local package recipe | `omarchy-kids-local-package` | `bb633d619f22dd77924e9bfebf04ce3168157724` |
| ArchISO submodule | detached | `424e78130db2af6c1ceb55b442d7914b1109ff2b` |

### Fresh readiness and native package preflight

The final validation began from the exact Nix source snapshot `/nix/store/3hqwd7gffq0w6npj0xz429fxihxfrw2v-source`. All 39 Git-index entries matched that snapshot by path, bytes, symlink target, and executable bit.

| Command | Wall time | Result |
| --- | ---: | --- |
| `nix run .#doctor` | 1.941s | Exit 0; all 13 host, tool, firmware, model, and sibling checks passed. |
| `nix run .#iso-unit` | 6.305s | Exit 0; the ISO shell suites and all 63 Python tests passed. |
| `nix run .#check` | 3.648s | Exit 0; 58 library, four CLI, one inference, three package-asset, and 49 workspace-script tests passed. |
| `nix flake check` | 1.488s | Exit 0; all flake outputs evaluated and checks passed. |

A fresh native Arch container built and installed the package from that snapshot. Its sole archive was 23,034,122 bytes with SHA-256 `d096786a04114e1cb571b90266a6bae65a454ecd432bdee8157f681aea345fed`. Independent checks validated the exact four-source `.SRCINFO` and hashes, `pacman -Qip`, the complete 26-line `pacman -Qlp` inventory and 14-entry non-directory allowlist, root ownership and modes, the two unique ELF objects and closed interpreter/library dependencies, the stable `libonnxruntime.so.1` ABI path and `libonnxruntime.so.1` SONAME, all installed model/license/setup hashes, and installed 1×1 PNG inference with the pinned model and empty detections.

The first preflight validator followed both ONNX Runtime symlink aliases and therefore overcounted two ELF objects as four. That complete attempt is preserved. A fresh unique rerun changed only the independent scan to exclude symlinks and passed in 107.539 seconds; no production source or acceptance condition changed.

### Exact successful build, install, and acceptance

| Command | Wall time | Result |
| --- | ---: | --- |
| `nix run .#kids-iso-build` with tag `kids-demo-final-20260904-100917-980113195` | 374.367s | Exit 0; exactly one tagged ISO and one matching package archive were published, with no staging residue. |
| `nix run .#kids-iso-test -- "$kids_iso" --install-only --no-preview` | 404.744s | Exit 0; the real graphical configurator completed, the second bounded SSH bootstrap attempt succeeded, and the fresh base was saved. |
| `nix run .#kids-iso-test -- "$kids_iso" --reuse-base --sync-omarchy /home/jake/Projects/omarchy --no-preview` | 153.546s | Exit 0; shortcut smoke, normal installed acceptance, external Kids acceptance, collection, and shutdown passed. |

The install run is `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195/runs/20260904-063725`. Its 23 files include 21 screenshots, a nonempty pacman log that records both the requested package and `[ALPM] installed omarchy-kids-browser-filter-demo (0.1.0-1)`, and a timing document whose 14 phases all report `ok`. The first console/SSH attempt reached its explicit 120-second bound and retained its failure screenshot; the harness's second bounded attempt succeeded and retained its bootstrap screenshots.

The canonical acceptance run is `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195/runs/20260904-064741`. It contains 55 files and 49 screenshots: 35 files and 34 screenshots from normal Omarchy acceptance, exactly three external Kids evidence files, the overlay, host captures, and the collected installer log. Normal acceptance passed in 80 seconds and explicitly passed both `no failed system units` and `no failed user units`.

### Final immutable artifacts

| Artifact | Size | Mode | SHA-256 |
| --- | ---: | ---: | --- |
| `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195.iso` | 6,209,560,576 bytes | `0644` | `c76830249287bad5235fb9bfb0690a078203ab62cb34cb9387c28e2436dc7f76` |
| `/home/jake/Projects/omarchy-iso/release/local-packages/kids-demo-final-20260904-100917-980113195/omarchy-kids-browser-filter-demo-0.1.0-1-x86_64.pkg.tar.zst` | 23,034,995 bytes | `0644` | `54d8046e070e9f89e2fbb9c720c2128d5d722f15703e038420c0a99ea0d1edd2` |
| `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-final-20260904-100917-980113195/base.qcow2` | 6,527,582,208 bytes; 40 GiB virtual | `0644` | `982618b68b74b373ae987a12aec9bb612ed94ad9a02e42a0bdfe229a922f9510` |
| Installed base `OVMF_VARS.4m.fd`, before acceptance | 540,672 bytes | `0600` | `dc6a2a7c884c4a41a1232cdff043d16440e2066a7763f72f84fa4ae0a716e6b3` |
| Acceptance `run.qcow2` overlay | 198,901,760 bytes; 40 GiB virtual | `0644` | `c8c3b43ef437ad3321519ee9b94013214094ed31a3654af1869f565da6a3a57a` |

Plain, non-repairing `qemu-img check` with the exact QEMU 11.1.0 closure found no errors in the base or overlay. The base hash remained unchanged through overlay acceptance. The expectedly mutable OVMF vars ended with SHA-256 `cf2d126d724b8e8525cb33463786e1285304e1b68349f17a36a60fa2882f0d78` and retained mode `0600`.

### Independently validated installed evidence

The external guest test observed the installed package, checked every owned non-directory path against the complete allowlist, checked required ownership and modes, verified the exact two-file extension and ONNX Runtime symlink chain, and observed a new headed Chromium client. The final archive independently reproduces the exact 14-entry non-directory allowlist, while the installer and collected logs tie that archive name and version to the installed system.

The collected summary independently passed the complete strengthened predicate: Chromium `Chrome/152.0.7977.82`, ONNX Runtime `1.27.1`, model SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`, 17 intercepted, 16 continued, one replaced, zero unresolved, clean shutdown, and 2 ms reveal latency. The requested and actual hold were both 1,500 ms. Three hold samples contained 1,413,000 opaque `[17, 19, 24, 255]` pixels; the reveal sample contained 471,000 pixels, all 16 safe colors, and placeholder `[255, 0, 255, 255]`, while excluding original flagged color `[5, 0, 0, 255]`. All 17 DOM images had the exact expected 1×1 color, including the placeholder at index 5.

The metric stream contains exactly 34 valid JSON objects with only sorted keys `elapsed_micros`, `fixture_index`, `stage`, and `verdict`: 17 `inference`, 17 `policy`, 32 `allow`, and two `replace`, with exactly both stages per index and replacement only at index 5. The summary, metrics, external acceptance log, and collected installer log have SHA-256 values `02f29f308205195fafe1c66c84fd71301b4c2ef47e3039fbfe518c9842af1356`, `58a743c4d4836771de7872d19b23cc54d92a989bb7c3ecfc2699c70d56e6ca6c`, `d659ec0326a5e2da3a465cf36ed1df8d859ff4a754278ead46d7e5eaf2913c47`, and `495ee2a45c851a96dc9859f3ebaa3339a232d8f32fa55f7d205220d4375ff72a`, respectively.

All 461 files frozen immediately before the final build and all 548 initially inventoried prior artifact files passed final SHA-256 verification; all pre-existing leaf/file metadata was unchanged. No disposable Kids profile, Kids-launched Chromium process, QEMU process, or configured listener remained. Complete logs, inventories, hashes, predicates, package validation, timings, and transient diagnoses are preserved under `.superpowers/sdd/2026-09-03-kids-iso-controlled-demo-implementation/final-artifact-validation-artifacts/20260904-100917-980113195/`.

All artifacts and commits remain local. No push, fetch, fork, pull request, remote change, upload, artifact replacement, evidence deletion, or assertion weakening was performed.

## Superseded but successful first private installed Kids ISO validation

On 2026-09-04, a separate uniquely tagged Kids ISO was built from the reviewed local repositories, installed through the real Omarchy graphical configurator into a fresh QEMU/KVM base, and validated through both the normal installed Omarchy acceptance suite and the external Kids browser fixture. This is not the upstream baseline ISO above: it contains one private local package, `omarchy-kids-browser-filter-demo`, and exposes only the dedicated controlled-demo launcher.

This run proves Arch packaging, offline inclusion and installation, dedicated launcher execution, and the controlled 17-image fixture's response interception, opaque hold cover, one deterministic replacement, privacy-safe metric schema, detector/runtime identity, and clean shutdown in an installed Omarchy VM. It does not prove arbitrary-site browsing, pornography-classifier accuracy, adversarial or bypass resistance, a tamper-resistant cover, service supervision, safe default-browser policy, managed Chromium policy, or redistribution rights.

### Exact source identities

All four repositories were clean at the reviewed local heads before the build and remained on branches without upstream tracking. Existing remote definitions were inspected but not changed or contacted.

| Checkout | Branch | Revision used |
| --- | --- | --- |
| Omarchy Kids package, wrappers, and external acceptance | `managed-browser-filter` | `91e008d1698d71b4a24d0a2e04343a2996ff3169` |
| Omarchy ISO generic package/build/acceptance interfaces | `omarchy-kids-local-workflow` | `8cebe1100b99ebd7e0442fbef390acc70a076415` |
| Omarchy source packaged into the ISO | `omarchy-kids-iso-compat` | `5df95727eaaa072087cbe63d81eb2789ee5f0eec` |
| Local package recipe | `omarchy-kids-local-package` | `159a5ee42a49e0672fd7982064c508d0bf76a256` |
| ArchISO submodule | detached | `424e78130db2af6c1ceb55b442d7914b1109ff2b` |

### Exact successful commands and timings

The commands ran from the linked Kids worktree with `OMARCHY_PATH`, `OMARCHY_ISO_PATH`, and `OMARCHY_PKGS_PATH` unset so the reviewed sibling defaults were exercised.

| Command | Wall time | Result |
| --- | ---: | --- |
| `nix run .#doctor` | 1.730s | Exit 0; all 13 host, tool, firmware, model, and sibling checks passed. |
| `nix run .#iso-unit` | 5.532s | Exit 0; all ISO shell contracts and 63 Python tests passed. |
| `nix run .#check` | 3.433s | Exit 0; the final workspace suite included 47 Rust/browser-wrapper cases. |
| `nix flake check` | 1.370s | Exit 0; all flake outputs evaluated and checks passed. |
| `nix run .#kids-iso-build` | 382.349s | Exit 0; exactly one new tagged ISO and one matching package archive were produced. |
| `nix run .#kids-iso-test -- "$kids_iso" --install-only --no-preview` | 402.701s | Exit 0; the real configurator completed, SSH bootstrap succeeded on its second bounded attempt, and the fresh base was saved. |
| `nix run .#kids-iso-test -- "$kids_iso" --reuse-base --no-preview` | 154.126s | Exit 0; shortcut smoke, normal installed acceptance, external Kids acceptance, collection, and shutdown passed. |

The normal in-guest Omarchy acceptance portion reported 82 seconds and explicitly passed both `no failed system units` and `no failed user units`.

### Immutable build and VM artifacts

The wrapper-generated tag was `kids-demo-20260904-074211-170516200`. Before the build there were no `*-kids-demo-*.iso` files. The already validated baseline ISO and every pre-existing package-preflight file passed a post-build SHA-256 check, and the pre-existing package stat inventory remained identical.

| Artifact | Size | Mode | SHA-256 |
| --- | ---: | ---: | --- |
| `/home/jake/Projects/omarchy-iso/release/omarchy-2026.09.04-x86_64-kids-demo-20260904-074211-170516200.iso` | 6,209,560,576 bytes | `0644` | `1fd7379385b64c3746a8f32e94e63226766de19dfb6347f6ec6cbd28da641368` |
| `/home/jake/Projects/omarchy-iso/release/local-packages/kids-demo-20260904-074211-170516200/omarchy-kids-browser-filter-demo-0.1.0-1-x86_64.pkg.tar.zst` | 23,034,986 bytes | `0644` | `dd8b39035959d92192e3917cf3598e5b288a95415fccbe8fa5f29cab1db61749` |
| `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-20260904-074211-170516200/base.qcow2` | 6,440,747,008 bytes; 40 GiB virtual | `0644` | `9ee290aa2be07b827ad4a363aaaaeb75591554139665dead9f31bb9f69a7b3bf` |
| Installed base `OVMF_VARS.4m.fd`, before acceptance | 540,672 bytes | `0600` | `540c7ab885aaceeba3adaa6b5104451e63907e8a260795bea965f7c5d20439f0` |
| Acceptance `run.qcow2` overlay | 171,769,856 bytes; 40 GiB virtual | `0644` | `6347e70ccb07d862c9dfb7190ed3868e4ca24120d335644f316139d1ae377d9e` |

Plain, non-repairing `qemu-img check` using the exact QEMU 11.1.0 closure from the Kids test app found no errors in either the base or the acceptance overlay. The base SHA-256 remained unchanged after acceptance. The writable OVMF vars file was expectedly updated by the reuse-base boot and ended with SHA-256 `81c8e756ef08fd1bc422a86384c91fdd6c31539f3f7566ef7a626824a6cd1092` while retaining mode `0600`.

`pacman -Qip` identified the archive as private-evaluation package `omarchy-kids-browser-filter-demo` version `0.1.0-1`, with Chromium, GCC libraries, and glibc dependencies, 21.97 MiB compressed size, and 44.13 MiB installed size. Its 26-line `pacman -Qlp` manifest contains the dedicated launcher, desktop entry, exact two-file extension, pinned model, bundled ONNX Runtime, and license/notices files. The installed-system pacman log records `installed omarchy-kids-browser-filter-demo (0.1.0-1)`, and the collected installer log independently lists and installs the same package.

The install run is preserved at `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-20260904-074211-170516200/runs/20260904-035052`. Its 23 files include 21 installer/first-boot screenshots, nonempty pacman evidence, and an install-timing document in which all 14 phases report `ok`.

### Installed browser acceptance evidence

The canonical passing acceptance run is preserved at `/home/jake/Projects/omarchy-iso/test-runs/omarchy-2026.09.04-x86_64-kids-demo-20260904-074211-170516200/runs/20260904-040008`. It contains 55 files and 49 screenshots: 35 files/34 screenshots from normal Omarchy acceptance, the acceptance overlay and host harness captures, the collected install log, and exactly three external Kids evidence files.

| Evidence | Measured value |
| --- | --- |
| Chromium | `Chrome/152.0.7977.82`; the guest test observed a new headed Chromium client for this launcher invocation |
| ONNX Runtime | `1.27.1` |
| Installed model SHA-256 | `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f` |
| Response results | 17 intercepted, 16 continued, one replaced, zero unresolved |
| Shutdown | Clean; the guest cleanup assertion passed and no launched Chromium, disposable profile, QEMU process, or configured listener remained afterward |
| Reveal | 2 ms after all required responses settled |
| Cover hold | 1,500 ms requested and actual; three screenshots sampled 1,413,000 covered pixels |
| Revealed pixels | One screenshot sampled 471,000 pixels; all 16 safe fixture colors and magenta placeholder `[255, 0, 255, 255]` were present, while original flagged color `[5, 0, 0, 255]` was absent |
| DOM | 17 one-pixel fixture images; index 5 had placeholder RGBA `[255, 0, 255, 255]` |
| Metrics | Exactly 34 valid JSONL objects: 17 `inference` and 17 `policy`; every object had exactly `elapsed_micros`, `fixture_index`, `stage`, and `verdict` |

The full summary predicate was re-run independently against `browser-filter-summary.json`, not inferred from the suite exit code. The summary, metrics, and external `acceptance.log` were nonempty and have SHA-256 values `02f29f308205195fafe1c66c84fd71301b4c2ef47e3039fbfe518c9842af1356`, `ca5e29a7a71d7ec087ff3df05de930f83329fd8c834843a9ad483fb173a98482`, and `2ff622a2cbf2e3271979b9ac72013c3d886d87f7fc4a588772262f8d56149e42`, respectively. The external log also records the installed-package, headed-client, fixture-contract, exact-metric-schema, and cleanup passes.

Complete command logs, timings, identities, before/after inventories, package metadata/manifests, qcow checks, hashes, and independent predicates are preserved under `.superpowers/sdd/2026-09-03-kids-iso-controlled-demo-implementation/task-5-artifacts/`.

### Execution notes and retries

No production source changed during this build/install/acceptance task, no assertion was weakened, and no failed command was relabeled as a pass.

- The first SSH console-bootstrap attempt reached its 120-second bound and saved `failure-first-boot-ssh-timeout-1.png`. The harness's bounded second attempt succeeded, saved `success-first-boot-06-bootstrap-complete.png`, collected the install logs, and shut down cleanly. Both outcomes remain preserved.
- The host shell did not expose `qemu-img`, and the default development shell did not add it, so two attempted read-only qcow checks exited `127`. The successful checks used the exact QEMU 11.1.0 store path already supplied to the test app; this changed no disk and required no download or production edit.
- An initial post-build inventory command was rejected before execution because it contained a temporary-file cleanup command disallowed by the execution boundary. It was replaced with a non-destructive stat/hash comparison whose inputs and outputs remain preserved.
- The external package-ownership checks emitted warnings because package sync databases are intentionally absent in the installed offline test system. Each exact owning-package assertion still succeeded, and the external suite exited zero.

All artifacts and commits remain local. No push, fetch, fork, pull request, remote change, upload, or evidence deletion was performed.

## Persistent managed-browser host smoke

On 2026-09-04, the new persistent `browse` path ran outside the VM against an ephemeral loopback page containing one harmless static PNG and one response labeled as PNG whose bytes were deliberately undecodable. Chromium `152.0.7977.75` and ONNX Runtime `1.27.1` loaded the pinned model SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`. Closing the supervised tab and, in a separate run, delivering SIGINT directly to the controller each returned exit zero with the same privacy-safe result: two intercepted, one continued, one replaced, one failed closed, zero unresolved, and clean shutdown. The startup page was also closed and counted as one blocked extra page. No managed Chromium process remained.

The smoke also surfaced and fixed two fail-safe orchestration defects before handoff: initial navigation had to run concurrently with paused response handling, and primary-page event streams end just before the global target-destroyed event during normal tab closure. A self-review then restricted successful manual-mode analysis to static JPEG and non-animated PNG so animated or partially analyzed formats are replaced. This is pipeline evidence for a driveable supervised session, not evidence of classifier accuracy or production child safety.

## What remains unproven

- Semantic parity with the upstream Python reference remains unverified because no checked-in reference golden was established. The verified model hash/runtime execution and colored-pixel preprocessing regressions are real evidence, but they are not a reference-parity gate.
- NudeNet accuracy, false-positive rate, recall, and threshold calibration were not evaluated. No real-world explicit-content corpus was used.
- Real-world pornography blocking and adversarial robustness were not tested. A hostile page can use content types, rendering paths, timing, or cover manipulation outside this controlled fixture.
- The NudeNet model's suitability, training-data provenance, and license for product distribution remain unresolved; upstream metadata conflicts and the weights lack sufficiently clear separate terms.
- Direct video-stream classification, canvas, WebGL, CSS background images, `data:` URLs, `blob:` URLs, service-worker-controlled responses, browser cache variants, back/forward cache, and many dynamically rendered paths are outside this proof. Host tests do prove that replacing a flagged image disables video and media controls connected to that image without blocking unrelated video.
- Only the supervised Chromium process is covered. Other browsers, native applications, Chromium-internal pages, and unsupervised network paths are not filtered.
- The page cover is a pipeline-spike mechanism and is not tamper-resistant against hostile page script.
- The experiment uses an ephemeral loopback DevTools endpoint. Production would need a private inherited pipe or equivalently confined control channel.
- The benchmark uses a small repeating synthetic corpus and does not establish accuracy, content diversity, peak memory, long-run stability, or minimum supported hardware.
- The upstream baseline, private Kids controlled fixture, renamed adult-filter package/install path, and persistent loopback-navigation smoke are proven on this machine, but real-world arbitrary-site safety validation, filter-service startup, default-browser integration, and bypass-resistant enforcement remain unperformed.

The next milestone may treat the controlled interception architecture as demonstrated, but production work must not treat this result as a pornography-classification validation or deployment approval.
