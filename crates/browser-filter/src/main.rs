use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use omarchy_kids_browser_filter::benchmark::{BenchmarkConfig, BenchmarkSummary, run_benchmark};
use omarchy_kids_browser_filter::inference::{
    DEFAULT_MAX_ENCODED_BYTES, DEFAULT_MAX_PIXELS, Detector, InferenceReport, MODEL_SHA256,
    ModelConfig,
};
use serde::Serialize;

#[cfg(test)]
use clap::CommandFactory;

#[derive(Parser)]
#[command(name = "omarchy-kids-browser-filter")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Infer {
        path: PathBuf,
    },
    Bench {
        #[arg(long, default_value_t = 20)]
        iterations: usize,
        #[arg(long, default_value_t = 3)]
        warmups: usize,
        #[arg(long)]
        json: bool,
    },
    Run,
}

#[derive(Serialize)]
struct InferOutput<'a> {
    model_sha256: &'static str,
    #[serde(flatten)]
    report: &'a InferenceReport,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Doctor => anyhow::bail!("doctor is not implemented"),
        Command::Infer { path } => infer(&path),
        Command::Bench {
            iterations,
            warmups,
            json,
        } => bench(iterations, warmups, json),
        Command::Run => anyhow::bail!("run is not implemented"),
    }
}

fn infer(path: &Path) -> Result<()> {
    let encoded = read_bounded(path, DEFAULT_MAX_ENCODED_BYTES)?;
    let mut detector = load_detector()?;
    let report = detector.detect(&encoded)?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(
        &mut stdout,
        &InferOutput {
            model_sha256: MODEL_SHA256,
            report: &report,
        },
    )?;
    writeln!(stdout)?;
    Ok(())
}

fn bench(iterations: usize, warmups: usize, json: bool) -> Result<()> {
    let mut detector = load_detector()?;
    let mut stdout = std::io::stdout().lock();
    write_benchmark_output(&mut stdout, iterations, warmups, json, |config| {
        run_benchmark(&mut detector, config)
    })
}

fn write_benchmark_output<W, F>(
    writer: &mut W,
    iterations: usize,
    warmups: usize,
    json: bool,
    mut run: F,
) -> Result<()>
where
    W: Write,
    F: FnMut(BenchmarkConfig) -> Result<BenchmarkSummary>,
{
    for mut config in BenchmarkConfig::default_workloads() {
        config.iterations = iterations;
        config.warmups = warmups;
        let summary = run(config)?;
        if json {
            serde_json::to_writer(&mut *writer, &summary)?;
            writeln!(writer)?;
        } else {
            writeln!(
                writer,
                "{} images: total workload p50/p90/p95 = {}/{}/{} us",
                summary.image_count,
                summary.total_workload_micros.p50,
                summary.total_workload_micros.p90,
                summary.total_workload_micros.p95
            )?;
        }
    }
    Ok(())
}

fn load_detector() -> Result<Detector> {
    Detector::load(ModelConfig {
        model_path: PathBuf::from(
            std::env::var_os("NUDENET_MODEL_PATH").context("NUDENET_MODEL_PATH is not set")?,
        ),
        runtime_path: PathBuf::from(
            std::env::var_os("ORT_DYLIB_PATH").context("ORT_DYLIB_PATH is not set")?,
        ),
        max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
        max_pixels: DEFAULT_MAX_PIXELS,
    })
}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let file = File::open(path).context("failed to open inference input")?;
    let read_limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut encoded = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut encoded)
        .context("failed to read inference input")?;
    anyhow::ensure!(
        encoded.len() <= max_bytes,
        "encoded image is {} bytes; limit is {max_bytes} bytes",
        encoded.len()
    );
    Ok(encoded)
}

#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}

// Production mutation caught: removing or renaming a benchmark option, or parsing its numeric
// value into the wrong field, would make the documented machine-readable command unusable.
#[test]
fn bench_cli_accepts_iteration_warmup_and_json_options() {
    let cli = Cli::try_parse_from([
        "omarchy-kids-browser-filter",
        "bench",
        "--iterations",
        "7",
        "--warmups",
        "2",
        "--json",
    ])
    .unwrap();

    let Command::Bench {
        iterations,
        warmups,
        json,
    } = cli.command
    else {
        panic!("expected bench command");
    };
    assert_eq!(iterations, 7);
    assert_eq!(warmups, 2);
    assert!(json);
}

// Production mutation caught: running fewer workloads, changing CLI defaults, or emitting one
// array/non-JSON payload would violate the four-record machine-readable benchmark contract.
#[test]
fn default_benchmark_output_is_four_jsonl_records_with_detector_metadata() {
    let mut output = Vec::new();
    let cli = Cli::try_parse_from(["omarchy-kids-browser-filter", "bench", "--json"]).unwrap();
    let Command::Bench {
        iterations,
        warmups,
        json,
    } = cli.command
    else {
        panic!("expected bench command");
    };

    write_benchmark_output(&mut output, iterations, warmups, json, |config| {
        Ok(synthetic_summary(config))
    })
    .unwrap();

    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records
            .iter()
            .map(|record| record["image_count"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 13, 19, 62]
    );
    for record in records {
        let mut keys = record
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
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
        );
        assert_eq!(record["warmups"], 3);
        assert_eq!(record["iterations"], 20);
        assert_eq!(record["cpu_model"], "Test CPU");
        assert_eq!(record["model_sha256"], "detector-model-sha");
        assert_eq!(record["onnx_runtime_version"], "9.8.7");
        assert_eq!(record["build_mode"], "debug");
        assert_eq!(
            record["inference_micros"],
            serde_json::json!({ "p50": 21, "p90": 22, "p95": 23 })
        );
    }
}

#[cfg(test)]
fn synthetic_summary(config: BenchmarkConfig) -> BenchmarkSummary {
    use omarchy_kids_browser_filter::benchmark::Percentiles;

    let timings = Percentiles {
        p50: 21,
        p90: 22,
        p95: 23,
    };
    BenchmarkSummary {
        cpu_model: "Test CPU".to_owned(),
        onnx_runtime_version: "9.8.7".to_owned(),
        model_sha256: "detector-model-sha".to_owned(),
        build_mode: "debug",
        image_count: config.image_count,
        warmups: config.warmups,
        iterations: config.iterations,
        encoded_bytes_median: 40,
        decode_micros: timings,
        preprocess_micros: timings,
        inference_micros: timings,
        postprocess_micros: timings,
        total_workload_micros: timings,
    }
}
