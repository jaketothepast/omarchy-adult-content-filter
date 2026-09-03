use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use base64::{Engine as _, prelude::BASE64_STANDARD};
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    cdp::browser_protocol::{
        fetch::{
            ContinueResponseParams, EnableParams, EventRequestPaused, FulfillRequestParams,
            GetResponseBodyParams, HeaderEntry, RequestId, RequestPattern, RequestStage,
        },
        network::ResourceType,
    },
};
use futures::StreamExt;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use url::{Host, Url};

use crate::{
    inference::{DEFAULT_MAX_ENCODED_BYTES, Detector, InferenceReport},
    metrics::{MetricRecord, MetricSink, MetricStage, MetricVerdict},
    policy::{Policy, Verdict},
};

const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct ExperimentConfig {
    fixture_url: Url,
    chromium_bin: PathBuf,
    profile_dir: PathBuf,
    extension_dir: PathBuf,
    image_count: usize,
    acquisition_timeout: Duration,
    inference_timeout: Duration,
    navigation_timeout: Duration,
}

impl ExperimentConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fixture_url: Url,
        chromium_bin: PathBuf,
        profile_dir: PathBuf,
        extension_dir: PathBuf,
        image_count: usize,
        acquisition_timeout: Duration,
        inference_timeout: Duration,
        navigation_timeout: Duration,
    ) -> Result<Self> {
        let loopback = match fixture_url.host() {
            Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
            Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
            _ => false,
        };
        anyhow::ensure!(
            fixture_url.scheme() == "http" && loopback,
            "fixture URL must use an HTTP loopback origin"
        );
        anyhow::ensure!(chromium_bin.is_file(), "Chromium executable does not exist");

        if let Some(home) = std::env::var_os("HOME") {
            let config = PathBuf::from(home).join(".config");
            anyhow::ensure!(
                profile_dir != config.join("chromium")
                    && profile_dir != config.join("google-chrome"),
                "Chromium profile must not be a default browser profile"
            );
        }
        anyhow::ensure!(
            profile_dir.is_dir() && fs::read_dir(&profile_dir)?.next().is_none(),
            "Chromium profile must be a new empty directory"
        );
        anyhow::ensure!(extension_dir.is_dir(), "extension directory does not exist");
        anyhow::ensure!(
            extension_dir.join("manifest.json").is_file(),
            "extension manifest does not exist"
        );
        anyhow::ensure!(
            (1..=100).contains(&image_count),
            "image count must be within 1..=100"
        );
        anyhow::ensure!(
            [acquisition_timeout, inference_timeout, navigation_timeout]
                .into_iter()
                .all(|timeout| !timeout.is_zero() && timeout <= MAX_OPERATION_TIMEOUT),
            "timeouts must be within 1ns..=30s"
        );

        Ok(Self {
            fixture_url,
            chromium_bin,
            profile_dir,
            extension_dir,
            image_count,
            acquisition_timeout,
            inference_timeout,
            navigation_timeout,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DomImageMetadata {
    pub index: usize,
    pub natural_width: u32,
    pub natural_height: u32,
    pub rgba: [u8; 4],
}

#[derive(Debug, Serialize)]
pub struct ExperimentSummary {
    pub chromium_version: String,
    pub intercepted: usize,
    pub continued: usize,
    pub replaced: usize,
    pub unresolved: usize,
    pub clean_shutdown: bool,
    pub dom_images: Vec<DomImageMetadata>,
}

pub struct BrowserExperiment<W> {
    detector: Detector,
    policy: Policy,
    metrics: MetricSink<W>,
}

impl<W: std::io::Write> BrowserExperiment<W> {
    pub fn new(detector: Detector, policy: Policy, metrics: MetricSink<W>) -> Self {
        Self {
            detector,
            policy,
            metrics,
        }
    }

    pub fn run(self, config: ExperimentConfig) -> Result<ExperimentSummary> {
        let runtime_shutdown_timeout = config.inference_timeout;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create browser experiment runtime")?;
        let result = runtime.block_on(run_experiment(self, config));
        runtime.shutdown_timeout(runtime_shutdown_timeout);
        result
    }
}

struct InferenceJob {
    encoded: Vec<u8>,
    result: oneshot::Sender<Result<InferenceReport>>,
}

struct InferenceWorker {
    sender: mpsc::Sender<InferenceJob>,
    task: tokio::task::JoinHandle<Result<()>>,
}

impl InferenceWorker {
    fn start(detector: Detector) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        let task = tokio::spawn(run_inference_worker(detector, receiver));
        Self { sender, task }
    }

    async fn detect(&self, encoded: Vec<u8>, deadline: Duration) -> Result<InferenceReport> {
        let (result, received) = oneshot::channel();
        tokio::time::timeout(deadline, async {
            self.sender
                .send(InferenceJob { encoded, result })
                .await
                .context("inference worker stopped before accepting an image")?;
            received
                .await
                .context("inference worker stopped before returning a verdict")?
        })
        .await
        .context("inference deadline elapsed")?
    }

    async fn shutdown(self, deadline: Duration) -> Result<()> {
        drop(self.sender);
        let mut task = self.task;
        match tokio::time::timeout(deadline, &mut task).await {
            Ok(joined) => {
                joined.context("inference worker task failed")??;
                Ok(())
            }
            Err(_) => {
                task.abort();
                Err(anyhow::anyhow!(
                    "inference worker shutdown deadline elapsed"
                ))
            }
        }
    }
}

async fn run_inference_worker(
    mut detector: Detector,
    mut receiver: mpsc::Receiver<InferenceJob>,
) -> Result<()> {
    while let Some(job) = receiver.recv().await {
        let completed = tokio::task::spawn_blocking(move || {
            let result = detector.detect(&job.encoded);
            (detector, job.result, result)
        })
        .await
        .context("blocking inference task failed")?;
        detector = completed.0;
        let _ = completed.1.send(completed.2);
    }
    Ok(())
}

async fn run_experiment<W: std::io::Write>(
    experiment: BrowserExperiment<W>,
    config: ExperimentConfig,
) -> Result<ExperimentSummary> {
    let BrowserExperiment {
        detector,
        policy,
        mut metrics,
    } = experiment;
    let browser_config = BrowserConfig::builder()
        .chrome_executable(&config.chromium_bin)
        .with_head()
        .user_data_dir(&config.profile_dir)
        .extension(config.extension_dir.display().to_string())
        .window_size(1280, 800)
        .launch_timeout(config.navigation_timeout)
        .request_timeout(config.acquisition_timeout)
        .respect_https_errors()
        .disable_cache()
        .build()
        .map_err(anyhow::Error::msg)?;
    let worker = InferenceWorker::start(detector);

    let launch = Browser::launch(browser_config).await;
    let result = match launch {
        Ok((browser, handler)) => {
            run_with_browser(browser, handler, &worker, &policy, &mut metrics, &config).await
        }
        Err(error) => Err(error).context("failed to launch headed Chromium"),
    };
    let worker_result = worker.shutdown(config.inference_timeout).await;

    match (result, worker_result) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(run_error), Err(worker_error)) => Err(anyhow::anyhow!(
            "{run_error:#}; inference worker cleanup failed: {worker_error:#}"
        )),
    }
}

async fn run_with_browser<W: std::io::Write>(
    mut browser: Browser,
    mut handler: chromiumoxide::Handler,
    worker: &InferenceWorker,
    policy: &Policy,
    metrics: &mut MetricSink<W>,
    config: &ExperimentConfig,
) -> Result<ExperimentSummary> {
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            event.context("Chromiumoxide handler failed")?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let ledger = Arc::new(Mutex::new(PauseLedger::default()));
    let run = tokio::time::timeout(
        config.navigation_timeout,
        run_page(
            &browser,
            worker,
            policy,
            metrics,
            config,
            Arc::clone(&ledger),
        ),
    )
    .await;
    let run_result = match run {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("navigation deadline elapsed")),
    };

    let cleanup_result =
        cleanup_browser(&mut browser, handler_task, config.acquisition_timeout).await;
    let unresolved = ledger
        .lock()
        .expect("pause ledger mutex poisoned")
        .unresolved_count();

    match (run_result, cleanup_result) {
        (Ok(mut summary), Ok(())) => {
            summary.unresolved = unresolved;
            summary.clean_shutdown = true;
            anyhow::ensure!(
                unresolved == 0,
                "{unresolved} response pauses remain unresolved"
            );
            Ok(summary)
        }
        (Err(error), Ok(())) => Err(anyhow::anyhow!(
            "{error:#}; unresolved response pauses: {unresolved}"
        )),
        (Ok(_), Err(error)) => Err(anyhow::anyhow!(
            "browser cleanup failed: {error:#}; unresolved response pauses: {unresolved}"
        )),
        (Err(run_error), Err(cleanup_error)) => Err(anyhow::anyhow!(
            "{run_error:#}; browser cleanup failed: {cleanup_error:#}; unresolved response pauses: {unresolved}"
        )),
    }
}

async fn cleanup_browser(
    browser: &mut Browser,
    mut handler_task: tokio::task::JoinHandle<Result<()>>,
    deadline: Duration,
) -> Result<()> {
    let close_result = tokio::time::timeout(deadline, browser.close()).await;
    if !matches!(close_result, Ok(Ok(_))) {
        let kill_result = tokio::time::timeout(deadline, async {
            match browser.kill().await {
                Some(result) => result,
                None => Ok(()),
            }
        })
        .await
        .context("Chromium kill deadline elapsed")?;
        kill_result.context("failed to kill Chromium after close failure")?;
    }

    tokio::time::timeout(deadline, browser.wait())
        .await
        .context("Chromium wait deadline elapsed")?
        .context("failed to wait for Chromium")?;

    match tokio::time::timeout(deadline, &mut handler_task).await {
        Ok(joined) => joined.context("Chromiumoxide handler task failed")??,
        Err(_) => {
            handler_task.abort();
            return Err(anyhow::anyhow!(
                "Chromiumoxide handler shutdown deadline elapsed"
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct RunCounts {
    intercepted: usize,
    continued: usize,
    replaced: usize,
}

async fn run_page<W: std::io::Write>(
    browser: &Browser,
    worker: &InferenceWorker,
    policy: &Policy,
    metrics: &mut MetricSink<W>,
    config: &ExperimentConfig,
    ledger: Arc<Mutex<PauseLedger>>,
) -> Result<ExperimentSummary> {
    let page = tokio::time::timeout(config.acquisition_timeout, browser.new_page("about:blank"))
        .await
        .context("about:blank page acquisition deadline elapsed")?
        .context("failed to create about:blank page")?;
    let chromium_version = tokio::time::timeout(config.acquisition_timeout, browser.version())
        .await
        .context("Chromium version acquisition deadline elapsed")?
        .context("failed to read Chromium version")?
        .product;
    let mut pauses = tokio::time::timeout(
        config.acquisition_timeout,
        page.event_listener::<EventRequestPaused>(),
    )
    .await
    .context("Fetch listener registration deadline elapsed")?
    .context("failed to register Fetch.requestPaused listener")?;
    tokio::time::timeout(
        config.acquisition_timeout,
        page.execute(fetch_enable_params()),
    )
    .await
    .context("Fetch.enable deadline elapsed")?
    .context("failed to enable image response interception")?;

    let mut navigation = Box::pin(page.goto(config.fixture_url.as_str()));
    let mut navigation_complete = false;
    let mut completed_images = 0;
    let mut counts = RunCounts::default();
    while completed_images < config.image_count {
        tokio::select! {
            navigation_result = &mut navigation, if !navigation_complete => {
                navigation_result.context("fixture navigation failed")?;
                navigation_complete = true;
            }
            event = pauses.next() => {
                let event = event.context("Fetch.requestPaused stream ended before all fixture images resolved")?;
                if process_pause(
                    &page,
                    event.as_ref(),
                    worker,
                    policy,
                    metrics,
                    config,
                    &ledger,
                    &mut counts,
                ).await? {
                    completed_images += 1;
                }
            }
        }
    }
    if !navigation_complete {
        navigation.await.context("fixture navigation failed")?;
    }

    let dom_images = page
        .evaluate(
            r#"Array.from(document.images).map((image, index) => {
                const canvas = document.createElement('canvas');
                canvas.width = 1;
                canvas.height = 1;
                const context = canvas.getContext('2d', { willReadFrequently: true });
                context.drawImage(image, 0, 0, 1, 1);
                return {
                    index,
                    natural_width: image.naturalWidth,
                    natural_height: image.naturalHeight,
                    rgba: Array.from(context.getImageData(0, 0, 1, 1).data),
                };
            })"#,
        )
        .await
        .context("failed to evaluate fixture DOM pixel metadata")?
        .into_value::<Vec<DomImageMetadata>>()
        .context("fixture DOM pixel metadata had an unexpected shape")?;
    anyhow::ensure!(
        dom_images.len() == config.image_count,
        "DOM reported {} images; expected {}",
        dom_images.len(),
        config.image_count
    );
    anyhow::ensure!(
        dom_images
            .iter()
            .all(|image| image.natural_width == 1 && image.natural_height == 1),
        "DOM reported an incomplete fixture image"
    );

    Ok(ExperimentSummary {
        chromium_version,
        intercepted: counts.intercepted,
        continued: counts.continued,
        replaced: counts.replaced,
        unresolved: ledger
            .lock()
            .expect("pause ledger mutex poisoned")
            .unresolved_count(),
        clean_shutdown: false,
        dom_images,
    })
}

struct PreparedDecision {
    replace: bool,
    terminal_response: bool,
    failure_after_resolution: Option<String>,
    metrics: Vec<MetricRecord>,
}

#[allow(clippy::too_many_arguments)]
async fn process_pause<W: std::io::Write>(
    page: &Page,
    event: &EventRequestPaused,
    worker: &InferenceWorker,
    policy: &Policy,
    metrics: &mut MetricSink<W>,
    config: &ExperimentConfig,
    ledger: &Arc<Mutex<PauseLedger>>,
    counts: &mut RunCounts,
) -> Result<bool> {
    ledger
        .lock()
        .expect("pause ledger mutex poisoned")
        .begin(&event.request_id)?;
    counts.intercepted += 1;

    let prepared = prepare_decision(page, event, worker, policy, config).await;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let fallback = tokio::time::timeout(
                config.acquisition_timeout,
                page.execute(replacement_response(event.request_id.clone())),
            )
            .await;
            if matches!(fallback, Ok(Ok(_))) {
                ledger
                    .lock()
                    .expect("pause ledger mutex poisoned")
                    .resolved(&event.request_id)?;
            }
            return Err(error.context("image response classification failed"));
        }
    };

    for record in &prepared.metrics {
        metrics
            .write(record)
            .context("failed to write privacy-safe response metric")?;
    }

    if prepared.replace {
        tokio::time::timeout(
            config.acquisition_timeout,
            page.execute(replacement_response(event.request_id.clone())),
        )
        .await
        .context("Fetch.fulfillRequest deadline elapsed")?
        .context("failed to fulfill flagged image response")?;
        counts.replaced += 1;
    } else {
        tokio::time::timeout(
            config.acquisition_timeout,
            page.execute(continue_response(event.request_id.clone())),
        )
        .await
        .context("Fetch.continueResponse deadline elapsed")?
        .context("failed to continue image response")?;
        counts.continued += 1;
    }
    ledger
        .lock()
        .expect("pause ledger mutex poisoned")
        .resolved(&event.request_id)?;

    if let Some(failure) = prepared.failure_after_resolution {
        anyhow::bail!(failure);
    }
    Ok(prepared.terminal_response)
}

async fn prepare_decision(
    page: &Page,
    event: &EventRequestPaused,
    worker: &InferenceWorker,
    policy: &Policy,
    config: &ExperimentConfig,
) -> Result<PreparedDecision> {
    let request_url = Url::parse(&event.request.url).context("paused image URL was invalid")?;
    anyhow::ensure!(
        request_url.origin() == config.fixture_url.origin(),
        "paused image left the fixture origin"
    );
    let has_response_error = event.response_error_reason.is_some();
    if !response_requires_body(event.response_status_code, has_response_error) {
        let status = event.response_status_code;
        let terminal_response = !matches!(status, Some(301 | 302 | 303 | 307 | 308));
        let failure_after_resolution = event
            .response_error_reason
            .as_ref()
            .map(|reason| format!("image response failed before body acquisition: {reason:?}"));
        return Ok(PreparedDecision {
            replace: false,
            terminal_response,
            failure_after_resolution,
            metrics: Vec::new(),
        });
    }

    let body = tokio::time::timeout(
        config.acquisition_timeout,
        page.execute(GetResponseBodyParams::new(event.request_id.clone())),
    )
    .await
    .context("Fetch.getResponseBody deadline elapsed")?
    .context("failed to acquire intercepted response body")?
    .result;
    let encoded = decode_response_body(&body.body, body.base64_encoded, DEFAULT_MAX_ENCODED_BYTES)?;
    let report = worker.detect(encoded, config.inference_timeout).await?;
    let fixture_index = fixture_index(&request_url)?;
    let inference_elapsed = report
        .decode_micros
        .saturating_add(report.preprocess_micros)
        .saturating_add(report.inference_micros)
        .saturating_add(report.postprocess_micros);
    let policy_started = Instant::now();
    let verdict = policy.decide(&request_url, &report);
    let policy_elapsed = u64::try_from(policy_started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let (replace, metric_verdict) = match verdict {
        Verdict::Allow => (false, MetricVerdict::Allow),
        Verdict::Replace { .. } => (true, MetricVerdict::Replace),
    };

    Ok(PreparedDecision {
        replace,
        terminal_response: true,
        failure_after_resolution: None,
        metrics: vec![
            MetricRecord {
                stage: MetricStage::Inference,
                verdict: if replace {
                    MetricVerdict::Replace
                } else {
                    MetricVerdict::Allow
                },
                fixture_index,
                elapsed_micros: inference_elapsed,
            },
            MetricRecord {
                stage: MetricStage::Policy,
                verdict: metric_verdict,
                fixture_index,
                elapsed_micros: policy_elapsed,
            },
        ],
    })
}

fn fixture_index(url: &Url) -> Result<usize> {
    let filename = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .context("fixture image URL has no filename")?;
    filename
        .strip_suffix(".png")
        .context("fixture image URL is not a PNG path")?
        .parse()
        .context("fixture image URL has a nonnumeric index")
}

fn replacement_headers() -> Vec<HeaderEntry> {
    vec![
        HeaderEntry::new("Content-Type", "image/png"),
        HeaderEntry::new("Cache-Control", "no-store"),
    ]
}

fn fetch_enable_params() -> EnableParams {
    EnableParams::builder()
        .pattern(
            RequestPattern::builder()
                .resource_type(ResourceType::Image)
                .request_stage(RequestStage::Response)
                .build(),
        )
        .build()
}

fn continue_response(request_id: RequestId) -> ContinueResponseParams {
    ContinueResponseParams::new(request_id)
}

fn replacement_response(request_id: RequestId) -> FulfillRequestParams {
    FulfillRequestParams::builder()
        .request_id(request_id)
        .response_code(200)
        .response_headers(replacement_headers())
        .body(BASE64_STANDARD.encode(placeholder_png()))
        .build()
        .expect("replacement response has all required fields")
}

fn placeholder_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, Rgb([255, 0, 255])));
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("writing a 1x1 in-memory PNG cannot fail");
    bytes.into_inner()
}

fn response_requires_body(response_status_code: Option<i64>, has_response_error: bool) -> bool {
    response_status_code == Some(200) && !has_response_error
}

fn decode_response_body(body: &str, base64_encoded: bool, max_bytes: usize) -> Result<Vec<u8>> {
    if base64_encoded {
        let padding = body
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'=')
            .count();
        let decoded_upper_bound = base64::decoded_len_estimate(body.len()).saturating_sub(padding);
        anyhow::ensure!(
            decoded_upper_bound <= max_bytes,
            "response body exceeds the {max_bytes} byte limit"
        );
        let decoded = BASE64_STANDARD.decode(body)?;
        anyhow::ensure!(
            decoded.len() <= max_bytes,
            "response body exceeds the {max_bytes} byte limit"
        );
        Ok(decoded)
    } else {
        anyhow::ensure!(
            body.len() <= max_bytes,
            "response body exceeds the {max_bytes} byte limit"
        );
        Ok(body.as_bytes().to_vec())
    }
}

#[derive(Default)]
struct PauseLedger {
    unresolved: HashSet<RequestId>,
}

impl PauseLedger {
    fn begin(&mut self, request_id: &RequestId) -> Result<()> {
        anyhow::ensure!(
            self.unresolved.insert(request_id.clone()),
            "request {} was observed more than once",
            request_id.as_ref()
        );
        Ok(())
    }

    fn resolved(&mut self, request_id: &RequestId) -> Result<()> {
        anyhow::ensure!(
            self.unresolved.remove(request_id),
            "request {} was resolved more than once",
            request_id.as_ref()
        );
        Ok(())
    }

    fn unresolved_count(&self) -> usize {
        self.unresolved.len()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, time::Duration};

    use base64::{Engine as _, prelude::BASE64_STANDARD};
    use chromiumoxide::cdp::browser_protocol::{
        fetch::{RequestId, RequestStage},
        network::ResourceType,
    };
    use image::GenericImageView;
    use tempfile::tempdir;
    use url::Url;

    use super::{
        ExperimentConfig, MAX_OPERATION_TIMEOUT, PauseLedger, continue_response,
        decode_response_body, fetch_enable_params, replacement_headers, replacement_response,
        response_requires_body,
    };

    fn existing_file(directory: &Path, name: &str) -> std::path::PathBuf {
        let path = directory.join(name);
        fs::write(&path, b"present").unwrap();
        path
    }

    // Production mutation caught: accepting an externally reachable origin would let this
    // controlled experiment navigate Chromium outside the loopback fixture boundary.
    #[test]
    fn rejects_non_loopback_fixture_urls() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile = root.path().join("profile");
        let extension = root.path().join("extension");
        fs::create_dir(&profile).unwrap();
        fs::create_dir(&extension).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://192.0.2.10:4000/").unwrap(),
            chromium,
            profile,
            extension,
            17,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "fixture URL must use an HTTP loopback origin"
        );
    }

    // Production mutation caught: allowing a nonempty profile would mix the experiment with
    // prior browser state instead of enforcing a fresh disposable profile.
    #[test]
    fn rejects_reused_chromium_profiles() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile = root.path().join("profile");
        let extension = root.path().join("extension");
        fs::create_dir(&profile).unwrap();
        fs::write(profile.join("History"), b"prior state").unwrap();
        fs::create_dir(&extension).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://127.0.0.1:4000/").unwrap(),
            chromium,
            profile,
            extension,
            17,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Chromium profile must be a new empty directory"
        );
    }

    // Production mutation caught: allowing Chromium's normal profile path would expose real
    // browsing state even if that directory happened to be empty on a new installation.
    #[test]
    fn rejects_default_chromium_profile() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile =
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".config/chromium");
        let extension = root.path().join("extension");
        fs::create_dir(&extension).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://127.0.0.1:4000/").unwrap(),
            chromium,
            profile,
            extension,
            17,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Chromium profile must not be a default browser profile"
        );
    }

    // Production mutation caught: deferring extension validation to Chromium would launch a
    // visibly unprotected page after a packaging or environment-path failure.
    #[test]
    fn rejects_missing_extension_directory() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile = root.path().join("profile");
        fs::create_dir(&profile).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://127.0.0.1:4000/").unwrap(),
            chromium,
            profile,
            root.path().join("missing-extension"),
            17,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "extension directory does not exist");
    }

    // Production mutation caught: accepting a directory without a manifest defers a deterministic
    // packaging error until Chromium launch, where no DevTools endpoint becomes available.
    #[test]
    fn rejects_extension_directory_without_a_manifest() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile = root.path().join("profile");
        let extension = root.path().join("extension");
        fs::create_dir(&profile).unwrap();
        fs::create_dir(&extension).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://127.0.0.1:4000/").unwrap(),
            chromium,
            profile,
            extension,
            17,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "extension manifest does not exist");
    }

    // Production mutation caught: widening either boundary would let the eager fixture corpus
    // and per-navigation work escape the experiment's fixed 1..=100 bound.
    #[test]
    fn rejects_image_counts_outside_one_through_one_hundred() {
        for count in [0, 101] {
            let root = tempdir().unwrap();
            let chromium = existing_file(root.path(), "chromium");
            let profile = root.path().join("profile");
            let extension = root.path().join("extension");
            fs::create_dir(&profile).unwrap();
            fs::create_dir(&extension).unwrap();
            fs::write(extension.join("manifest.json"), b"{}").unwrap();

            let error = ExperimentConfig::new(
                Url::parse("http://127.0.0.1:4000/").unwrap(),
                chromium,
                profile,
                extension,
                count,
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(30),
            )
            .unwrap_err();

            assert_eq!(error.to_string(), "image count must be within 1..=100");
        }
    }

    // Production mutation caught: accepting a zero or over-30-second wait would make acquisition,
    // inference, or navigation either immediately unusable or effectively unbounded.
    #[test]
    fn rejects_zero_and_over_thirty_second_operation_timeouts() {
        for (timeout_index, timeout) in [
            Duration::ZERO,
            MAX_OPERATION_TIMEOUT + Duration::from_millis(1),
        ]
        .into_iter()
        .flat_map(|timeout| (0..3).map(move |timeout_index| (timeout_index, timeout)))
        {
            let root = tempdir().unwrap();
            let chromium = existing_file(root.path(), "chromium");
            let profile = root.path().join("profile");
            let extension = root.path().join("extension");
            fs::create_dir(&profile).unwrap();
            fs::create_dir(&extension).unwrap();
            fs::write(extension.join("manifest.json"), b"{}").unwrap();
            let mut timeouts = [
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(30),
            ];
            timeouts[timeout_index] = timeout;

            let error = ExperimentConfig::new(
                Url::parse("http://127.0.0.1:4000/").unwrap(),
                chromium,
                profile,
                extension,
                17,
                timeouts[0],
                timeouts[1],
                timeouts[2],
            )
            .unwrap_err();

            assert_eq!(error.to_string(), "timeouts must be within 1ns..=30s");
        }
    }

    // Production mutation caught: forwarding an origin header or omitting no-store would leak
    // response metadata or allow the deterministic placeholder to enter Chromium's cache.
    #[test]
    fn replacement_uses_only_png_and_no_store_headers() {
        let headers = replacement_headers();
        let actual = headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            [("Content-Type", "image/png"), ("Cache-Control", "no-store"),]
        );
    }

    // Production mutation caught: omitting either Fetch qualifier or adding a catch-all pattern
    // would pause requests/documents instead of only image responses.
    #[test]
    fn fetch_configuration_has_one_image_response_stage_pattern() {
        let params = fetch_enable_params();
        let patterns = params.patterns.unwrap();

        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].url_pattern, None);
        assert_eq!(patterns[0].resource_type, Some(ResourceType::Image));
        assert_eq!(patterns[0].request_stage, Some(RequestStage::Response));
        assert_eq!(params.handle_auth_requests, None);
    }

    // Production mutation caught: attaching any optional override to an allowed response changes
    // the required argument-free continueResponse compatibility path.
    #[test]
    fn allowed_response_continues_without_response_overrides() {
        let params = continue_response(RequestId::new("allowed-1"));
        let encoded = serde_json::to_value(params).unwrap();

        assert_eq!(encoded, serde_json::json!({ "requestId": "allowed-1" }));
    }

    // Production mutation caught: returning non-PNG bytes, the wrong deterministic pixel, an
    // origin status, or extra protocol fields would make replacement unverifiable in the DOM.
    #[test]
    fn flagged_response_fulfills_a_magenta_png_with_only_required_fields() {
        let params = replacement_response(RequestId::new("flagged-1"));
        let encoded = serde_json::to_value(&params).unwrap();
        let encoded_body: String = params.body.unwrap().into();
        let body = BASE64_STANDARD.decode(encoded_body).unwrap();
        let image = image::load_from_memory(&body).unwrap();

        assert_eq!(params.response_code, 200);
        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 255, 255]);
        assert_eq!(
            encoded.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["body", "requestId", "responseCode", "responseHeaders"]
        );
    }

    // Production mutation caught: silently accepting a duplicate pause or duplicate resolution
    // would hide requests that were resolved zero or two times from the unresolved count.
    #[test]
    fn pause_ledger_enforces_one_begin_and_one_resolution_per_request_id() {
        let mut ledger = PauseLedger::default();
        let request_id = RequestId::new("pause-1");

        ledger.begin(&request_id).unwrap();
        assert_eq!(ledger.unresolved_count(), 1);
        assert_eq!(
            ledger.begin(&request_id).unwrap_err().to_string(),
            "request pause-1 was observed more than once"
        );
        ledger.resolved(&request_id).unwrap();
        assert_eq!(ledger.unresolved_count(), 0);
        assert_eq!(
            ledger.resolved(&request_id).unwrap_err().to_string(),
            "request pause-1 was resolved more than once"
        );
    }

    // Production mutation caught: requesting a body for redirects, 204/304, or response failures
    // violates Fetch.getResponseBody's response-stage preconditions.
    #[test]
    fn only_successful_http_200_responses_require_body_acquisition() {
        assert!(response_requires_body(Some(200), false));
        for status in [None, Some(204), Some(301), Some(304)] {
            assert!(!response_requires_body(status, false));
        }
        assert!(!response_requires_body(Some(200), true));
    }

    // Production mutation caught: treating Fetch's body string uniformly would either infer over
    // base64 text or corrupt an unencoded response before the detector sees it.
    #[test]
    fn response_body_decoder_handles_base64_and_plain_bytes() {
        assert_eq!(
            decode_response_body("AAEC", true, 3).unwrap(),
            vec![0, 1, 2]
        );
        assert_eq!(decode_response_body("plain", false, 5).unwrap(), b"plain");
    }

    // Production mutation caught: decoding before enforcing the encoded-size ceiling would allow
    // a CDP response body to allocate beyond the detector's fixed ingress limit.
    #[test]
    fn response_body_decoder_rejects_content_over_the_byte_bound() {
        let error = decode_response_body("AAECAw==", true, 3).unwrap_err();

        assert_eq!(error.to_string(), "response body exceeds the 3 byte limit");
    }
}
