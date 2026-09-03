# Managed-browser host experiment results

## Result

On 2026-09-03, all three reproduction commands completed successfully on the machine described below. The controlled headed-browser run intercepted 17 harmless PNG responses, continued 16, replaced the deterministically marked fixture at index 5, revealed only after every response was resolved, shut Chromium down cleanly, and removed its disposable profile.

This result proves the controlled response-interception, cover, and replacement plumbing. It does not validate NudeNet accuracy or real-world pornography blocking. The flagged route is a deterministic policy fixture, not an accuracy test: the model runs for timing, then the fixture marker overrides the final verdict so the replacement branch always executes.

## Reproduction commands

Run from the repository root:

```bash
nix flake check
nix run .#bench -- --iterations 20 --warmups 3 --json
nix run .#run -- --images 17 --flagged-index 5 --hold-millis 500 --assert-no-flash --json
```

These are the exact commands used for this result. `nix flake check` finished with `all checks passed!`. The benchmark and browser commands emitted newline-delimited JSON; the measurements below are transcribed from that fresh output.

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

## What remains unproven

- NudeNet accuracy, false-positive rate, recall, and threshold calibration were not evaluated. No real-world explicit-content corpus was used.
- Real-world pornography blocking and adversarial robustness were not tested. A hostile page can use content types, rendering paths, timing, or cover manipulation outside this controlled fixture.
- The NudeNet model's suitability, training-data provenance, and license for product distribution remain unresolved; upstream metadata conflicts and the weights lack sufficiently clear separate terms.
- Video, canvas, WebGL, CSS background images, `data:` URLs, `blob:` URLs, service-worker-controlled responses, browser cache variants, back/forward cache, and dynamically loaded content are outside this proof.
- Only the supervised Chromium process is covered. Other browsers, native applications, Chromium-internal pages, and unsupervised network paths are not filtered.
- The page cover is a pipeline-spike mechanism and is not tamper-resistant against hostile page script.
- The experiment uses an ephemeral loopback DevTools endpoint. Production would need a private inherited pipe or equivalently confined control channel.
- The benchmark uses a small repeating synthetic corpus and does not establish accuracy, content diversity, peak memory, long-run stability, or minimum supported hardware.
- ISO packaging, managed-policy installation, service startup, and QEMU/VM acceptance integration have not been performed.

The next milestone may treat the controlled interception architecture as demonstrated, but production work must not treat this result as a pornography-classification validation or deployment approval.
