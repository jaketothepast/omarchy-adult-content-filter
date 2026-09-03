use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
    Infer { path: PathBuf },
    Bench,
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
        Command::Bench => anyhow::bail!("bench is not implemented"),
        Command::Run => anyhow::bail!("run is not implemented"),
    }
}

fn infer(path: &Path) -> Result<()> {
    let encoded = read_bounded(path, DEFAULT_MAX_ENCODED_BYTES)?;
    let mut detector = Detector::load(ModelConfig {
        model_path: PathBuf::from(
            std::env::var_os("NUDENET_MODEL_PATH").context("NUDENET_MODEL_PATH is not set")?,
        ),
        runtime_path: PathBuf::from(
            std::env::var_os("ORT_DYLIB_PATH").context("ORT_DYLIB_PATH is not set")?,
        ),
        max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
        max_pixels: DEFAULT_MAX_PIXELS,
    })?;
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
