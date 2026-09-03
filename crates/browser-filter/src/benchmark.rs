use std::time::Instant;

use anyhow::{Context, Result};
use image::{Rgb, RgbImage, codecs::jpeg::JpegEncoder};
use serde::Serialize;

use crate::inference::{Detector, DetectorMetadata, InferenceReport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchmarkConfig {
    pub image_count: usize,
    pub warmups: usize,
    pub iterations: usize,
}

impl BenchmarkConfig {
    pub fn default_workloads() -> [Self; 4] {
        [1, 13, 19, 62].map(|image_count| Self {
            image_count,
            warmups: 3,
            iterations: 20,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Percentiles {
    pub p50: u64,
    pub p90: u64,
    pub p95: u64,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkSummary {
    pub cpu_model: String,
    pub onnx_runtime_version: String,
    pub model_sha256: String,
    pub build_mode: &'static str,
    pub image_count: usize,
    pub warmups: usize,
    pub iterations: usize,
    pub encoded_bytes_median: u64,
    pub decode_micros: Percentiles,
    pub preprocess_micros: Percentiles,
    pub inference_micros: Percentiles,
    pub postprocess_micros: Percentiles,
    pub total_workload_micros: Percentiles,
}

pub fn run_benchmark(detector: &mut Detector, config: BenchmarkConfig) -> Result<BenchmarkSummary> {
    run_benchmark_with_engine(detector, config, cpu_model()?)
}

trait DetectionEngine {
    fn detect(&mut self, encoded: &[u8]) -> Result<InferenceReport>;
    fn metadata(&self) -> &DetectorMetadata;
}

impl DetectionEngine for Detector {
    fn detect(&mut self, encoded: &[u8]) -> Result<InferenceReport> {
        Detector::detect(self, encoded)
    }

    fn metadata(&self) -> &DetectorMetadata {
        Detector::metadata(self)
    }
}

struct BenchmarkMeasurements {
    decode_micros: Vec<u64>,
    preprocess_micros: Vec<u64>,
    inference_micros: Vec<u64>,
    postprocess_micros: Vec<u64>,
    total_workload_micros: Vec<u64>,
}

fn run_benchmark_with_engine<E: DetectionEngine>(
    detector: &mut E,
    config: BenchmarkConfig,
    cpu_model: String,
) -> Result<BenchmarkSummary> {
    anyhow::ensure!(
        config.image_count > 0,
        "image_count must be greater than zero"
    );
    anyhow::ensure!(
        config.iterations > 0,
        "iterations must be greater than zero"
    );
    let images = generate_jpegs(config.image_count)?;
    let encoded_bytes = images
        .iter()
        .map(|encoded| u64::try_from(encoded.len()).unwrap_or(u64::MAX))
        .collect::<Vec<_>>();
    let measurements = measure_benchmark(detector, &images, config)?;
    let metadata = detector.metadata();

    Ok(BenchmarkSummary {
        cpu_model,
        onnx_runtime_version: metadata.runtime_version.clone(),
        model_sha256: metadata.model_sha256.clone(),
        build_mode: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        image_count: config.image_count,
        warmups: config.warmups,
        iterations: config.iterations,
        encoded_bytes_median: nearest_rank_percentiles(&encoded_bytes)[0],
        decode_micros: percentiles(&measurements.decode_micros),
        preprocess_micros: percentiles(&measurements.preprocess_micros),
        inference_micros: percentiles(&measurements.inference_micros),
        postprocess_micros: percentiles(&measurements.postprocess_micros),
        total_workload_micros: percentiles(&measurements.total_workload_micros),
    })
}

fn measure_benchmark<E: DetectionEngine>(
    detector: &mut E,
    images: &[Vec<u8>],
    config: BenchmarkConfig,
) -> Result<BenchmarkMeasurements> {
    anyhow::ensure!(
        images.len() == config.image_count,
        "benchmark image corpus does not match image_count"
    );
    let sample_count = config
        .image_count
        .checked_mul(config.iterations)
        .context("benchmark sample count overflowed")?;

    for _ in 0..config.warmups {
        for encoded in images {
            detector.detect(encoded)?;
        }
    }

    let mut decode_micros = Vec::with_capacity(sample_count);
    let mut preprocess_micros = Vec::with_capacity(sample_count);
    let mut inference_micros = Vec::with_capacity(sample_count);
    let mut postprocess_micros = Vec::with_capacity(sample_count);
    let mut total_workload_micros = Vec::with_capacity(config.iterations);
    for _ in 0..config.iterations {
        let workload_started = Instant::now();
        for encoded in images {
            let report = detector.detect(encoded)?;
            decode_micros.push(report.decode_micros);
            preprocess_micros.push(report.preprocess_micros);
            inference_micros.push(report.inference_micros);
            postprocess_micros.push(report.postprocess_micros);
        }
        total_workload_micros.push(elapsed_micros(workload_started));
    }

    Ok(BenchmarkMeasurements {
        decode_micros,
        preprocess_micros,
        inference_micros,
        postprocess_micros,
        total_workload_micros,
    })
}

fn generate_jpegs(image_count: usize) -> Result<Vec<Vec<u8>>> {
    (0..image_count)
        .map(|index| {
            let image = RgbImage::from_fn(1280, 720, |x, y| {
                let index = u32::try_from(index).unwrap_or(u32::MAX);
                Rgb([
                    ((x / 32 + index) % 16 * 16) as u8,
                    ((y / 24 + index * 3) % 16 * 16) as u8,
                    ((x / 64 + y / 36 + index * 7) % 16 * 16) as u8,
                ])
            });
            let mut encoded = Vec::new();
            JpegEncoder::new_with_quality(&mut encoded, 80)
                .encode_image(&image)
                .context("failed to encode benchmark JPEG")?;
            Ok(encoded)
        })
        .collect()
}

fn cpu_model() -> Result<String> {
    let cpuinfo =
        std::fs::read_to_string("/proc/cpuinfo").context("failed to read CPU metadata")?;
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find_map(|(key, value)| (key.trim() == "model name").then(|| value.trim().to_owned()))
        .filter(|model| !model.is_empty())
        .context("CPU model name is unavailable")
}

fn elapsed_micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn percentiles(samples: &[u64]) -> Percentiles {
    let [p50, p90, p95] = nearest_rank_percentiles(samples);
    Percentiles { p50, p90, p95 }
}

fn nearest_rank_percentiles(samples: &[u64]) -> [u64; 3] {
    assert!(
        !samples.is_empty(),
        "percentiles require at least one sample"
    );
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    [50, 90, 95].map(|percentile| {
        let rank = (sorted.len() * percentile).div_ceil(100);
        sorted[rank - 1]
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, io::Cursor};

    use image::{ImageFormat, ImageReader};

    use crate::inference::{DetectorMetadata, InferenceReport};

    use super::{
        BenchmarkConfig, BenchmarkSummary, DetectionEngine, Percentiles, generate_jpegs,
        measure_benchmark, nearest_rank_percentiles, run_benchmark_with_engine,
    };

    struct FakeDetector {
        metadata: DetectorMetadata,
        detections: u64,
    }

    impl DetectionEngine for FakeDetector {
        fn detect(&mut self, encoded: &[u8]) -> anyhow::Result<InferenceReport> {
            self.detections += 1;
            Ok(InferenceReport {
                detections: Vec::new(),
                encoded_bytes: encoded.len(),
                width: 1280,
                height: 720,
                decode_micros: self.detections,
                preprocess_micros: self.detections + 10,
                inference_micros: self.detections + 20,
                postprocess_micros: self.detections + 30,
            })
        }

        fn metadata(&self) -> &DetectorMetadata {
            &self.metadata
        }
    }

    fn fake_detector() -> FakeDetector {
        FakeDetector {
            metadata: DetectorMetadata {
                model_sha256: "detector-model-sha".to_owned(),
                runtime_path: "/nix/store/example/lib/libonnxruntime.so.misleading".into(),
                runtime_version: "9.8.7".to_owned(),
            },
            detections: 0,
        }
    }

    // Production mutation caught: leaving samples unsorted, using a zero-based rank directly, or
    // rounding down would change one of the hand-derived nearest-rank results for ten samples.
    #[test]
    fn calculates_nearest_rank_p50_p90_and_p95_for_fixed_values() {
        let samples = [90, 10, 100, 20, 80, 30, 70, 40, 60, 50];

        assert_eq!(nearest_rank_percentiles(&samples), [50, 90, 100]);
    }

    // Production mutation caught: changing a workload size, warmup count, or measured iteration
    // count would benchmark a different scenario than the four planned browser page sizes.
    #[test]
    fn default_workloads_use_planned_image_counts_warmups_and_iterations() {
        assert_eq!(
            BenchmarkConfig::default_workloads(),
            [
                BenchmarkConfig {
                    image_count: 1,
                    warmups: 3,
                    iterations: 20,
                },
                BenchmarkConfig {
                    image_count: 13,
                    warmups: 3,
                    iterations: 20,
                },
                BenchmarkConfig {
                    image_count: 19,
                    warmups: 3,
                    iterations: 20,
                },
                BenchmarkConfig {
                    image_count: 62,
                    warmups: 3,
                    iterations: 20,
                },
            ]
        );
    }

    // Production mutation caught: changing dimensions or format, introducing nondeterminism, or
    // reusing one payload for every index would no longer produce the planned harmless corpus.
    #[test]
    fn generates_distinct_deterministic_1280_by_720_jpegs_in_memory() {
        let first_run = generate_jpegs(2).unwrap();
        let second_run = generate_jpegs(2).unwrap();

        assert_eq!(first_run, second_run);
        assert_ne!(first_run[0], first_run[1]);
        for encoded in first_run {
            let reader = ImageReader::new(Cursor::new(encoded))
                .with_guessed_format()
                .unwrap();
            assert_eq!(reader.format(), Some(ImageFormat::Jpeg));
            assert_eq!(reader.into_dimensions().unwrap(), (1280, 720));
        }
    }

    // Production mutation caught: including warmup reports, collecting one sample per workload,
    // or replacing the stateful detector would change these literal samples and cardinalities.
    #[test]
    fn measurement_reuses_one_detector_and_excludes_warmups_from_stage_samples() {
        let mut detector = fake_detector();
        let config = BenchmarkConfig {
            image_count: 2,
            warmups: 1,
            iterations: 3,
        };
        let images = [vec![1], vec![2]];

        let measurements = measure_benchmark(&mut detector, &images, config).unwrap();

        assert_eq!(detector.detections, 8);
        assert_eq!(measurements.decode_micros, [3, 4, 5, 6, 7, 8]);
        assert_eq!(measurements.preprocess_micros.len(), 6);
        assert_eq!(measurements.inference_micros.len(), 6);
        assert_eq!(measurements.postprocess_micros.len(), 6);
        assert_eq!(measurements.total_workload_micros.len(), 3);
    }

    // Production mutation caught: reading model/runtime identity from constants or ambient
    // environment instead of the benchmarked detector would replace these fake identities.
    #[test]
    fn benchmark_summary_uses_identity_from_the_benchmarked_detector() {
        let mut detector = fake_detector();
        let summary = run_benchmark_with_engine(
            &mut detector,
            BenchmarkConfig {
                image_count: 1,
                warmups: 0,
                iterations: 1,
            },
            "Test CPU".to_owned(),
        )
        .unwrap();

        assert_eq!(summary.cpu_model, "Test CPU");
        assert_eq!(summary.model_sha256, "detector-model-sha");
        assert_eq!(summary.onnx_runtime_version, "9.8.7");
    }

    // Production mutation caught: adding image bytes, paths, tensors, detections, or other fields
    // to the serialized result would cross the benchmark's privacy-safe output boundary.
    #[test]
    fn summary_serializes_only_timing_and_runtime_metadata() {
        let timings = Percentiles {
            p50: 10,
            p90: 20,
            p95: 30,
        };
        let summary = BenchmarkSummary {
            cpu_model: "Example CPU".to_owned(),
            onnx_runtime_version: "1.27.1".to_owned(),
            model_sha256: "model-hash".to_owned(),
            build_mode: "release",
            image_count: 13,
            warmups: 3,
            iterations: 20,
            encoded_bytes_median: 40,
            decode_micros: timings,
            preprocess_micros: timings,
            inference_micros: timings,
            postprocess_micros: timings,
            total_workload_micros: timings,
        };

        let value = serde_json::to_value(summary).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            [
                "build_mode",
                "cpu_model",
                "decode_micros",
                "encoded_bytes_median",
                "image_count",
                "inference_micros",
                "iterations",
                "model_sha256",
                "onnx_runtime_version",
                "postprocess_micros",
                "preprocess_micros",
                "total_workload_micros",
                "warmups",
            ]
            .map(str::to_owned)
            .into_iter()
            .collect()
        );
        assert_eq!(
            value["inference_micros"],
            serde_json::json!({ "p50": 10, "p90": 20, "p95": 30 })
        );
    }
}
