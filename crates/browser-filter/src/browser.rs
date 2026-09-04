use std::{
    collections::HashSet,
    fs,
    future::Future,
    io::Cursor,
    net::Ipv4Addr,
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
        page::{CaptureScreenshotFormat, CaptureScreenshotParams, Viewport},
    },
};
use futures::StreamExt;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use url::{Host, Url};

use crate::{
    inference::{DEFAULT_MAX_ENCODED_BYTES, Detector, DetectorMetadata, InferenceReport},
    metrics::{MetricRecord, MetricSink, MetricStage, MetricVerdict},
    policy::{Policy, Verdict},
};

const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BUFFERED_METRICS: usize = 2 * 100;
const MAX_REVEAL_LATENCY: Duration = Duration::from_millis(500);
const COVER_RGBA: [u8; 4] = [17, 19, 24, 255];
const PLACEHOLDER_RGBA: [u8; 4] = [255, 0, 255, 255];

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
    expected_flagged_index: Option<usize>,
    no_flash_hold: Option<Duration>,
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
        let controlled_origin = matches!(fixture_url.host(), Some(Host::Ipv4(address)) if address == Ipv4Addr::LOCALHOST);
        anyhow::ensure!(
            fixture_url.scheme() == "http" && controlled_origin,
            "fixture URL must use the controlled http://127.0.0.1 origin"
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
            expected_flagged_index: None,
            no_flash_hold: None,
        })
    }

    pub fn with_expected_flagged_index(mut self, flagged_index: usize) -> Result<Self> {
        anyhow::ensure!(
            flagged_index < self.image_count,
            "flagged index must identify a configured image"
        );
        self.expected_flagged_index = Some(flagged_index);
        Ok(self)
    }

    pub fn with_no_flash_assertion(mut self, hold_duration: Duration) -> Result<Self> {
        anyhow::ensure!(
            !hold_duration.is_zero() && hold_duration <= self.navigation_timeout,
            "no-flash hold must be within 1ns..=navigation timeout"
        );
        self.no_flash_hold = Some(hold_duration);
        Ok(self)
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
    pub onnx_runtime_version: String,
    pub model_sha256: String,
    pub intercepted: usize,
    pub continued: usize,
    pub replaced: usize,
    pub unresolved: usize,
    pub clean_shutdown: bool,
    pub reveal_latency_millis: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_flash_assertion: Option<NoFlashAssertionSummary>,
    pub dom_images: Vec<DomImageMetadata>,
}

#[derive(Debug, Serialize)]
pub struct NoFlashAssertionSummary {
    pub requested_hold_millis: u64,
    pub actual_hold_millis: u64,
    pub hold_screenshot_count: usize,
    pub hold_sampled_pixels: usize,
    pub reveal_screenshot_count: usize,
    pub reveal_sampled_pixels: usize,
    pub cover_rgba: [u8; 4],
    pub safe_fixture_colors_present: usize,
    pub placeholder_rgba: [u8; 4],
    pub placeholder_color_present: bool,
    pub original_flagged_rgba: [u8; 4],
    pub original_flagged_color_absent: bool,
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

pub(crate) struct HeadedDetectorSession {
    metadata: DetectorMetadata,
    worker: InferenceWorker,
}

impl HeadedDetectorSession {
    pub(crate) fn start(detector: Detector) -> Self {
        let metadata = detector.metadata().clone();
        let worker = InferenceWorker::start(detector);
        Self { metadata, worker }
    }

    pub(crate) async fn detect(
        &self,
        encoded: Vec<u8>,
        deadline: Duration,
    ) -> Result<InferenceReport> {
        self.worker.detect(encoded, deadline).await
    }

    pub(crate) fn metadata(&self) -> &DetectorMetadata {
        &self.metadata
    }

    #[allow(clippy::too_many_arguments)]
    fn summary(
        &self,
        chromium_version: String,
        counts: RunCounts,
        unresolved: usize,
        reveal_latency_millis: u64,
        no_flash_assertion: Option<NoFlashAssertionSummary>,
        dom_images: Vec<DomImageMetadata>,
    ) -> ExperimentSummary {
        experiment_summary(
            chromium_version,
            &self.metadata,
            counts,
            unresolved,
            reveal_latency_millis,
            no_flash_assertion,
            dom_images,
        )
    }

    pub(crate) async fn shutdown(self, deadline: Duration) -> Result<()> {
        self.worker.shutdown(deadline).await
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
    let detector_session = HeadedDetectorSession::start(detector);
    let mut metric_buffer = MetricBuffer::default();

    let launch = Browser::launch(browser_config).await;
    let result = match launch {
        Ok((browser, handler)) => {
            run_with_browser(
                browser,
                handler,
                &detector_session,
                &policy,
                &mut metric_buffer,
                &config,
            )
            .await
        }
        Err(error) => Err(error).context("failed to launch headed Chromium"),
    };
    let worker_result = detector_session.shutdown(config.inference_timeout).await;
    let metrics_result = flush_metrics(metric_buffer, &mut metrics);

    let mut summary = None;
    let mut errors = Vec::new();
    match result {
        Ok(value) => summary = Some(value),
        Err(error) => errors.push(format!("{error:#}")),
    }
    if let Err(error) = worker_result {
        errors.push(format!("inference worker cleanup failed: {error:#}"));
    }
    if let Err(error) = metrics_result {
        errors.push(format!("telemetry flush failed: {error:#}"));
    }
    if errors.is_empty() {
        Ok(summary.expect("a successful browser run returns a summary"))
    } else {
        Err(anyhow::anyhow!(errors.join("; ")))
    }
}

async fn run_with_browser(
    mut browser: Browser,
    mut handler: chromiumoxide::Handler,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    metrics: &mut MetricBuffer,
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
            detector_session,
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

    let cleanup_result = cleanup_browser(&mut browser, handler_task, config.acquisition_timeout)
        .await
        .into_result();
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

pub(crate) async fn cleanup_browser(
    browser: &mut Browser,
    mut handler_task: tokio::task::JoinHandle<Result<()>>,
    deadline: Duration,
) -> CleanupReport {
    let mut operations = BrowserCleanupOperations {
        browser,
        handler_task: &mut handler_task,
    };
    perform_cleanup(&mut operations, deadline).await
}

trait CleanupOperations {
    async fn close(&mut self) -> Result<()>;
    async fn kill(&mut self) -> Result<()>;
    async fn wait(&mut self) -> Result<()>;
    async fn join_handler(&mut self) -> Result<()>;
    fn abort_handler(&mut self);
    async fn reap_aborted_handler(&mut self) -> Result<()>;
}

struct BrowserCleanupOperations<'a> {
    browser: &'a mut Browser,
    handler_task: &'a mut tokio::task::JoinHandle<Result<()>>,
}

impl CleanupOperations for BrowserCleanupOperations<'_> {
    async fn close(&mut self) -> Result<()> {
        self.browser
            .close()
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    }

    async fn kill(&mut self) -> Result<()> {
        match self.browser.kill().await {
            Some(result) => result.map_err(anyhow::Error::from),
            None => Ok(()),
        }
    }

    async fn wait(&mut self) -> Result<()> {
        self.browser
            .wait()
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    }

    async fn join_handler(&mut self) -> Result<()> {
        (&mut *self.handler_task)
            .await
            .context("Chromiumoxide handler task failed")?
    }

    fn abort_handler(&mut self) {
        self.handler_task.abort();
    }

    async fn reap_aborted_handler(&mut self) -> Result<()> {
        match (&mut *self.handler_task).await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(error).context("aborted Chromiumoxide handler task failed"),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct CleanupReport {
    errors: Vec<String>,
    forced_reap: bool,
}

impl CleanupReport {
    fn is_clean(&self) -> bool {
        self.errors.is_empty() && !self.forced_reap
    }

    #[cfg(test)]
    fn forced_reap(&self) -> bool {
        self.forced_reap
    }

    pub(crate) fn into_result(self) -> Result<()> {
        if self.is_clean() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(self.errors.join("; ")))
        }
    }

    fn error(&mut self, error: impl Into<String>) {
        self.errors.push(error.into());
    }
}

async fn perform_cleanup<O: CleanupOperations>(
    operations: &mut O,
    deadline: Duration,
) -> CleanupReport {
    let mut report = CleanupReport::default();
    match tokio::time::timeout(deadline, operations.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            report.forced_reap = true;
            report.error(format!("failed to close Chromium gracefully: {error:#}"));
        }
        Err(_) => {
            report.forced_reap = true;
            report.error("Chromium graceful close deadline elapsed");
        }
    }

    if report.forced_reap {
        match tokio::time::timeout(deadline, operations.kill()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                report.error(format!(
                    "failed to kill Chromium after close failure: {error:#}"
                ));
            }
            Err(_) => report.error("Chromium kill deadline elapsed"),
        }
    }

    match tokio::time::timeout(deadline, operations.wait()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report.error(format!("failed to wait for Chromium: {error:#}")),
        Err(_) => report.error("Chromium wait deadline elapsed"),
    }

    match tokio::time::timeout(deadline, operations.join_handler()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report.error(format!("Chromiumoxide handler shutdown failed: {error:#}")),
        Err(_) => {
            report.error("Chromiumoxide handler shutdown deadline elapsed");
            operations.abort_handler();
            match tokio::time::timeout(deadline, operations.reap_aborted_handler()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => report.error(format!(
                    "failed to reap aborted Chromiumoxide handler: {error:#}"
                )),
                Err(_) => report.error("aborted Chromiumoxide handler reap deadline elapsed"),
            }
        }
    }

    report
}

#[derive(Default)]
struct RunCounts {
    intercepted: usize,
    continued: usize,
    replaced: usize,
}

fn experiment_summary(
    chromium_version: String,
    detector_metadata: &DetectorMetadata,
    counts: RunCounts,
    unresolved: usize,
    reveal_latency_millis: u64,
    no_flash_assertion: Option<NoFlashAssertionSummary>,
    dom_images: Vec<DomImageMetadata>,
) -> ExperimentSummary {
    ExperimentSummary {
        chromium_version,
        onnx_runtime_version: detector_metadata.runtime_version.clone(),
        model_sha256: detector_metadata.model_sha256.clone(),
        intercepted: counts.intercepted,
        continued: counts.continued,
        replaced: counts.replaced,
        unresolved,
        clean_shutdown: false,
        reveal_latency_millis,
        no_flash_assertion,
        dom_images,
    }
}

struct NoFlashCapture {
    requested_hold: Duration,
    actual_hold: Duration,
    hold_screenshot_count: usize,
    hold_sampled_pixels: usize,
    flagged_index: Option<usize>,
    reveal: Option<RevealScreenshotEvidence>,
}

impl NoFlashCapture {
    fn new(requested_hold: Duration) -> Self {
        Self {
            requested_hold,
            actual_hold: Duration::ZERO,
            hold_screenshot_count: 0,
            hold_sampled_pixels: 0,
            flagged_index: None,
            reveal: None,
        }
    }

    async fn hold_flagged_response(
        &mut self,
        page: &Page,
        flagged_index: usize,
        screenshot_timeout: Duration,
    ) -> Result<()> {
        anyhow::ensure!(
            self.flagged_index.is_none(),
            "more than one flagged response entered the no-flash hold"
        );
        self.flagged_index = Some(flagged_index);
        let started = Instant::now();
        let sample_offsets = [
            Duration::ZERO,
            self.requested_hold / 2,
            self.requested_hold.mul_f64(0.8),
        ];
        for offset in sample_offsets {
            tokio::time::sleep_until(tokio::time::Instant::from_std(started + offset)).await;
            let screenshot = capture_screenshot(page, screenshot_timeout).await?;
            let sampled_pixels = assert_cover_screenshot(&screenshot)?;
            self.hold_sampled_pixels = self
                .hold_sampled_pixels
                .checked_add(sampled_pixels)
                .context("held screenshot pixel count overflowed")?;
            self.hold_screenshot_count += 1;
        }
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            started + self.requested_hold,
        ))
        .await;
        self.actual_hold = started.elapsed();
        anyhow::ensure!(
            self.hold_screenshot_count >= 3,
            "no-flash hold captured fewer than three screenshots"
        );
        Ok(())
    }

    async fn capture_reveal(
        &mut self,
        page: &Page,
        image_count: usize,
        screenshot_timeout: Duration,
    ) -> Result<()> {
        let flagged_index = self
            .flagged_index
            .context("flagged response was not held before reveal")?;
        let screenshot = capture_screenshot(page, screenshot_timeout).await?;
        self.reveal = Some(assert_reveal_screenshot(
            &screenshot,
            image_count,
            flagged_index,
        )?);
        Ok(())
    }

    fn into_summary(self) -> Result<NoFlashAssertionSummary> {
        let flagged_index = self
            .flagged_index
            .context("no-flash assertion did not observe a flagged response")?;
        let reveal = self
            .reveal
            .context("no-flash assertion did not capture the revealed page")?;
        Ok(NoFlashAssertionSummary {
            requested_hold_millis: duration_millis(self.requested_hold),
            actual_hold_millis: duration_millis(self.actual_hold),
            hold_screenshot_count: self.hold_screenshot_count,
            hold_sampled_pixels: self.hold_sampled_pixels,
            reveal_screenshot_count: 1,
            reveal_sampled_pixels: reveal.sampled_pixels,
            cover_rgba: COVER_RGBA,
            safe_fixture_colors_present: reveal.safe_fixture_colors_present,
            placeholder_rgba: PLACEHOLDER_RGBA,
            placeholder_color_present: reveal.placeholder_color_present,
            original_flagged_rgba: fixture_rgba(flagged_index),
            original_flagged_color_absent: reveal.original_flagged_color_absent,
        })
    }
}

async fn capture_screenshot(page: &Page, deadline: Duration) -> Result<Vec<u8>> {
    tokio::time::timeout(deadline, async {
        let region = page
            .evaluate(
                "({ width: document.documentElement.clientWidth, height: document.documentElement.clientHeight })",
            )
            .await
            .context("failed to measure screenshot content region")?
            .into_value::<ScreenshotRegion>()
            .context("screenshot content region had an unexpected shape")?;
        anyhow::ensure!(
            region.width > 0 && region.height > 0,
            "screenshot content region must be positive"
        );
        page.screenshot(
            CaptureScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .clip(Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: f64::from(region.width),
                    height: f64::from(region.height),
                    scale: 1.0,
                })
                .from_surface(true)
                .capture_beyond_viewport(false)
                .build(),
        )
        .await
        .context("failed to capture in-memory page screenshot")
    })
    .await
    .context("screenshot acquisition deadline elapsed")?
}

#[derive(Deserialize)]
struct ScreenshotRegion {
    width: u32,
    height: u32,
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug)]
struct ValidatedRevealLatency(Duration);

impl ValidatedRevealLatency {
    fn between(final_response_settled_at: Instant, readiness_applied_at: Instant) -> Result<Self> {
        let duration = readiness_applied_at
            .checked_duration_since(final_response_settled_at)
            .context("readiness completion preceded final response settlement")?;
        anyhow::ensure!(
            duration <= MAX_REVEAL_LATENCY,
            "reveal latency of {}ns exceeds the {}ns limit",
            duration.as_nanos(),
            MAX_REVEAL_LATENCY.as_nanos()
        );
        Ok(Self(duration))
    }

    fn summary_millis(&self) -> u64 {
        duration_millis(self.0)
    }
}

async fn finish_reveal_after_response<Readiness, ReadinessResult, PostReadiness, PostFuture>(
    final_response_settled_at: Instant,
    readiness: Readiness,
    post_readiness: PostReadiness,
) -> Result<ValidatedRevealLatency>
where
    Readiness: Future<Output = Result<ReadinessResult>>,
    PostReadiness: FnOnce(ReadinessResult) -> PostFuture,
    PostFuture: Future<Output = Result<()>>,
{
    let readiness_result = readiness.await?;
    let readiness_applied_at = Instant::now();
    let latency = ValidatedRevealLatency::between(final_response_settled_at, readiness_applied_at)?;
    post_readiness(readiness_result).await?;
    Ok(latency)
}

async fn run_page(
    browser: &Browser,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    metrics: &mut MetricBuffer,
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

    let expected_flagged_index = config
        .expected_flagged_index
        .context("expected flagged fixture index is not configured")?;
    let mut navigation = Box::pin(page.goto(config.fixture_url.as_str()));
    let mut navigation_complete = false;
    let mut completed_images = 0;
    let mut final_response_settled_at = None;
    let mut counts = RunCounts::default();
    let mut no_flash = config.no_flash_hold.map(NoFlashCapture::new);
    while completed_images < config.image_count {
        tokio::select! {
            navigation_result = &mut navigation, if !navigation_complete => {
                navigation_result.context("fixture navigation failed")?;
                navigation_complete = true;
            }
            event = pauses.next() => {
                let event = event.context("Fetch.requestPaused stream ended before all fixture images resolved")?;
                let outcome = process_pause(
                    &page,
                    event.as_ref(),
                    detector_session,
                    policy,
                    metrics,
                    config,
                    &ledger,
                    &mut counts,
                    &mut no_flash,
                ).await?;
                if outcome.terminal_response {
                    completed_images += 1;
                    if completed_images == config.image_count {
                        final_response_settled_at = Some(outcome.settled_at);
                    }
                }
            }
        }
    }
    if !navigation_complete {
        navigation.await.context("fixture navigation failed")?;
    }

    let reveal_latency = finish_reveal_after_response(
        final_response_settled_at
            .context("final fixture response settlement time was not recorded")?,
        async {
            tokio::time::timeout(
                config.acquisition_timeout,
                page.evaluate(
                    r#"(() => {
                        document.documentElement.setAttribute('data-omarchy-kids-ready', '');
                        return document.documentElement.hasAttribute('data-omarchy-kids-ready');
                    })()"#,
                ),
            )
            .await
            .context("fixture reveal deadline elapsed")?
            .context("failed to set fixture readiness")
        },
        |readiness_result| async {
            let ready = readiness_result
                .into_value::<bool>()
                .context("fixture readiness result had an unexpected shape")?;
            anyhow::ensure!(ready, "fixture readiness attribute was not set");
            if let Some(assertion) = no_flash.as_mut() {
                assertion
                    .capture_reveal(&page, config.image_count, config.acquisition_timeout)
                    .await?;
            }
            Ok(())
        },
    )
    .await?;
    let reveal_latency_millis = reveal_latency.summary_millis();

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
    assert_dom_image_colors(&dom_images, expected_flagged_index)?;
    let no_flash_assertion = no_flash.map(NoFlashCapture::into_summary).transpose()?;

    Ok(detector_session.summary(
        chromium_version,
        counts,
        ledger
            .lock()
            .expect("pause ledger mutex poisoned")
            .unresolved_count(),
        reveal_latency_millis,
        no_flash_assertion,
        dom_images,
    ))
}

struct PreparedDecision {
    replace: bool,
    terminal_response: bool,
    failure_after_resolution: Option<String>,
    metrics: Vec<MetricRecord>,
}

struct PauseOutcome {
    terminal_response: bool,
    settled_at: Instant,
}

#[derive(Default)]
struct MetricBuffer {
    records: Vec<MetricRecord>,
}

impl MetricBuffer {
    fn record(&mut self, records: Vec<MetricRecord>) -> Result<()> {
        anyhow::ensure!(
            self.records.len().saturating_add(records.len()) <= MAX_BUFFERED_METRICS,
            "response metrics exceed the {MAX_BUFFERED_METRICS}-record bound"
        );
        self.records.extend(records);
        Ok(())
    }
}

fn resolve_and_buffer_metrics(
    ledger: &mut PauseLedger,
    request_id: &RequestId,
    metrics: &mut MetricBuffer,
    records: Vec<MetricRecord>,
) -> Result<()> {
    ledger.resolved(request_id)?;
    metrics.record(records)
}

fn finish_response_after_cdp(
    terminal_response: bool,
    bookkeeping: impl FnOnce() -> Result<()>,
) -> Result<PauseOutcome> {
    // This is the response-settlement boundary: the successful CDP continue/fulfill has just
    // returned. Count mutation, ledger removal, metric buffering, caller dispatch, and reveal work
    // all happen after this stamp and are therefore charged to response-to-readiness latency.
    let settled_at = Instant::now();
    bookkeeping()?;
    Ok(PauseOutcome {
        terminal_response,
        settled_at,
    })
}

fn flush_metrics<W: std::io::Write>(metrics: MetricBuffer, sink: &mut MetricSink<W>) -> Result<()> {
    for record in &metrics.records {
        sink.write(record)
            .context("failed to write buffered privacy-safe response metric")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_pause(
    page: &Page,
    event: &EventRequestPaused,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    metrics: &mut MetricBuffer,
    config: &ExperimentConfig,
    ledger: &Arc<Mutex<PauseLedger>>,
    counts: &mut RunCounts,
    no_flash: &mut Option<NoFlashCapture>,
) -> Result<PauseOutcome> {
    ledger
        .lock()
        .expect("pause ledger mutex poisoned")
        .begin(&event.request_id)?;
    counts.intercepted += 1;

    let prepared = prepare_decision(page, event, detector_session, policy, config).await;
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

    let outcome = if prepared.replace {
        let request_url = Url::parse(&event.request.url).context("paused image URL was invalid")?;
        let replaced_index = fixture_index(&request_url)?;
        anyhow::ensure!(
            Some(replaced_index) == config.expected_flagged_index,
            "replacement targeted fixture image {replaced_index}, expected {}",
            config
                .expected_flagged_index
                .context("expected flagged fixture index is not configured")?
        );
        if let Some(assertion) = no_flash {
            assertion
                .hold_flagged_response(page, replaced_index, config.acquisition_timeout)
                .await?;
        }
        tokio::time::timeout(
            config.acquisition_timeout,
            page.execute(replacement_response(event.request_id.clone())),
        )
        .await
        .context("Fetch.fulfillRequest deadline elapsed")?
        .context("failed to fulfill flagged image response")?;
        finish_response_after_cdp(prepared.terminal_response, || {
            counts.replaced += 1;
            resolve_and_buffer_metrics(
                &mut ledger.lock().expect("pause ledger mutex poisoned"),
                &event.request_id,
                metrics,
                prepared.metrics,
            )
        })
    } else {
        tokio::time::timeout(
            config.acquisition_timeout,
            page.execute(continue_response(event.request_id.clone())),
        )
        .await
        .context("Fetch.continueResponse deadline elapsed")?
        .context("failed to continue image response")?;
        finish_response_after_cdp(prepared.terminal_response, || {
            counts.continued += 1;
            resolve_and_buffer_metrics(
                &mut ledger.lock().expect("pause ledger mutex poisoned"),
                &event.request_id,
                metrics,
                prepared.metrics,
            )
        })
    }?;

    if let Some(failure) = prepared.failure_after_resolution {
        anyhow::bail!(failure);
    }
    Ok(outcome)
}

async fn prepare_decision(
    page: &Page,
    event: &EventRequestPaused,
    detector_session: &HeadedDetectorSession,
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
    let report = detector_session
        .detect(encoded, config.inference_timeout)
        .await?;
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

pub(crate) fn fetch_enable_params() -> EnableParams {
    EnableParams::builder()
        .pattern(
            RequestPattern::builder()
                .resource_type(ResourceType::Image)
                .request_stage(RequestStage::Response)
                .build(),
        )
        .build()
}

pub(crate) fn continue_response(request_id: RequestId) -> ContinueResponseParams {
    ContinueResponseParams::new(request_id)
}

pub(crate) fn replacement_response(request_id: RequestId) -> FulfillRequestParams {
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

#[derive(Debug, PartialEq, Eq)]
struct RevealScreenshotEvidence {
    sampled_pixels: usize,
    safe_fixture_colors_present: usize,
    placeholder_color_present: bool,
    original_flagged_color_absent: bool,
}

fn assert_cover_screenshot(encoded: &[u8]) -> Result<usize> {
    let screenshot = image::load_from_memory(encoded)
        .context("failed to decode held screenshot")?
        .to_rgba8();
    for (x, y, pixel) in screenshot.enumerate_pixels() {
        anyhow::ensure!(
            pixel.0 == COVER_RGBA,
            "held screenshot pixel at ({x}, {y}) is {:?}, expected {:?}",
            pixel.0,
            COVER_RGBA
        );
    }
    Ok(screenshot.pixels().len())
}

fn assert_reveal_screenshot(
    encoded: &[u8],
    image_count: usize,
    flagged_index: usize,
) -> Result<RevealScreenshotEvidence> {
    let screenshot = image::load_from_memory(encoded)
        .context("failed to decode revealed screenshot")?
        .to_rgba8();
    let pixels = screenshot.pixels().map(|pixel| pixel.0).collect::<Vec<_>>();

    let mut safe_fixture_colors_present = 0;
    for index in 0..image_count {
        if index == flagged_index {
            continue;
        }
        let expected = fixture_rgba(index);
        anyhow::ensure!(
            pixels.contains(&expected),
            "revealed screenshot does not contain safe fixture color {expected:?}"
        );
        safe_fixture_colors_present += 1;
    }
    let placeholder_color_present = pixels.contains(&PLACEHOLDER_RGBA);
    anyhow::ensure!(
        placeholder_color_present,
        "revealed screenshot does not contain placeholder color {PLACEHOLDER_RGBA:?}"
    );
    let original_flagged = fixture_rgba(flagged_index);
    let original_flagged_color_absent = !pixels.contains(&original_flagged);
    anyhow::ensure!(
        original_flagged_color_absent,
        "revealed screenshot contains original flagged color {original_flagged:?}"
    );

    Ok(RevealScreenshotEvidence {
        sampled_pixels: pixels.len(),
        safe_fixture_colors_present,
        placeholder_color_present,
        original_flagged_color_absent,
    })
}

fn assert_dom_image_colors(images: &[DomImageMetadata], flagged_index: usize) -> Result<()> {
    for (position, image) in images.iter().enumerate() {
        anyhow::ensure!(
            image.index == position,
            "DOM image position {position} reported index {}",
            image.index
        );
        let expected = if image.index == flagged_index {
            PLACEHOLDER_RGBA
        } else {
            fixture_rgba(image.index)
        };
        anyhow::ensure!(
            image.rgba == expected,
            "DOM image {} rendered {:?}, expected {expected:?}",
            image.index,
            image.rgba
        );
    }
    Ok(())
}

fn fixture_rgba(index: usize) -> [u8; 4] {
    [index as u8, (index >> 8) as u8, (index >> 16) as u8, 255]
}

fn response_requires_body(response_status_code: Option<i64>, has_response_error: bool) -> bool {
    response_status_code == Some(200) && !has_response_error
}

pub(crate) fn decode_response_body(
    body: &str,
    base64_encoded: bool,
    max_bytes: usize,
) -> Result<Vec<u8>> {
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
    seen: HashSet<RequestId>,
    unresolved: HashSet<RequestId>,
}

impl PauseLedger {
    fn begin(&mut self, request_id: &RequestId) -> Result<()> {
        anyhow::ensure!(
            self.seen.insert(request_id.clone()),
            "request {} was observed more than once",
            request_id.as_ref()
        );
        self.unresolved.insert(request_id.clone());
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
    use std::{
        fs,
        io::{Cursor, Write},
        path::Path,
        time::{Duration, Instant},
    };

    use base64::{Engine as _, prelude::BASE64_STANDARD};
    use chromiumoxide::cdp::browser_protocol::{
        fetch::{RequestId, RequestStage},
        network::ResourceType,
    };
    use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
    use tempfile::tempdir;
    use url::Url;

    use super::{
        CleanupOperations, DomImageMetadata, ExperimentConfig, HeadedDetectorSession,
        MAX_OPERATION_TIMEOUT, MetricBuffer, PauseLedger, RunCounts, ValidatedRevealLatency,
        assert_cover_screenshot, assert_dom_image_colors, assert_reveal_screenshot,
        continue_response, decode_response_body, fetch_enable_params, finish_response_after_cdp,
        finish_reveal_after_response, flush_metrics, perform_cleanup, replacement_headers,
        replacement_response, resolve_and_buffer_metrics, response_requires_body,
    };
    use crate::inference::{
        DEFAULT_MAX_ENCODED_BYTES, DEFAULT_MAX_PIXELS, Detector, MODEL_SHA256, ModelConfig,
    };
    use crate::metrics::{MetricRecord, MetricSink, MetricStage, MetricVerdict};

    // Production mutations caught: deriving runtime identity from a selected library filename, or
    // losing the cloned identity when moving the exact Detector into its worker, would misreport a
    // successful headed session even though the loaded runtime API reports a different version.
    #[tokio::test]
    async fn headed_summary_uses_detector_reported_identity() {
        let source_runtime = std::env::var_os("ORT_DYLIB_PATH").unwrap();
        let model_path = std::env::var_os("NUDENET_MODEL_PATH").unwrap().into();
        let temporary_directory = tempdir().unwrap();
        let misleading_runtime = temporary_directory.path().join("libonnxruntime.so.9.8.7");
        if fs::hard_link(&source_runtime, &misleading_runtime).is_err() {
            fs::copy(&source_runtime, &misleading_runtime).unwrap();
        }
        let detector = Detector::load(ModelConfig {
            model_path,
            runtime_path: misleading_runtime,
            max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
            max_pixels: DEFAULT_MAX_PIXELS,
        })
        .unwrap();
        let session = HeadedDetectorSession::start(detector);

        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])))
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let report = session
            .detect(encoded.into_inner(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!((report.width, report.height), (1, 1));

        let summary = session.summary(
            "Chromium 140".to_owned(),
            RunCounts::default(),
            0,
            125,
            None,
            Vec::new(),
        );
        let value = serde_json::to_value(summary).unwrap();

        assert_eq!(value["onnx_runtime_version"], "1.27.1");
        assert_eq!(value["model_sha256"], MODEL_SHA256);
        assert!(value.get("runtime_path").is_none());
        session.shutdown(Duration::from_secs(5)).await.unwrap();
    }

    // Production mutation caught: adding permissions, executable resources, undeclared files, or
    // remote CSS loads would widen this permissionless declarative cover into executable behavior.
    #[test]
    fn extension_contract_declares_a_permissionless_document_start_cover() {
        let extension = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../browser-extension");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(extension.join("manifest.json")).unwrap()).unwrap();

        let mut manifest_keys = manifest.as_object().unwrap().keys().collect::<Vec<_>>();
        manifest_keys.sort_unstable();
        assert_eq!(
            manifest_keys,
            ["content_scripts", "manifest_version", "name", "version"]
        );
        assert_eq!(manifest["manifest_version"], 3);
        assert!(manifest.get("permissions").is_none());
        assert!(manifest.get("host_permissions").is_none());
        assert!(manifest.get("background").is_none());
        assert!(manifest.get("web_accessible_resources").is_none());
        let content_scripts = manifest["content_scripts"].as_array().unwrap();
        assert_eq!(content_scripts.len(), 1);
        let mut content_script_keys = content_scripts[0]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>();
        content_script_keys.sort_unstable();
        assert_eq!(
            content_script_keys,
            [
                "all_frames",
                "css",
                "match_about_blank",
                "matches",
                "run_at"
            ]
        );
        assert_eq!(
            content_scripts[0]["matches"],
            serde_json::json!(["<all_urls>"])
        );
        assert_eq!(content_scripts[0]["css"], serde_json::json!(["cover.css"]));
        assert_eq!(content_scripts[0]["run_at"], "document_start");
        assert_eq!(content_scripts[0]["all_frames"], true);
        assert_eq!(content_scripts[0]["match_about_blank"], true);
        assert!(content_scripts[0].get("js").is_none());

        let mut extension_files = fs::read_dir(&extension)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        extension_files.sort_unstable();
        assert_eq!(extension_files, ["cover.css", "manifest.json"]);

        let css = fs::read_to_string(extension.join("cover.css")).unwrap();
        assert_eq!(
            css,
            concat!(
                "html:not([data-omarchy-kids-ready]) {\n",
                "  background: #111318 !important;\n",
                "}\n",
                "\n",
                "html:not([data-omarchy-kids-ready]) > * {\n",
                "  visibility: hidden !important;\n",
                "}\n",
            )
        );
    }

    fn existing_file(directory: &Path, name: &str) -> std::path::PathBuf {
        let path = directory.join(name);
        fs::write(&path, b"present").unwrap();
        path
    }

    fn metric_record(fixture_index: usize) -> MetricRecord {
        MetricRecord {
            stage: MetricStage::Inference,
            verdict: MetricVerdict::Allow,
            fixture_index,
            elapsed_micros: 1,
        }
    }

    fn screenshot_png(width: u32, pixels: Vec<Rgba<u8>>) -> Vec<u8> {
        let height = u32::try_from(pixels.len()).unwrap() / width;
        let image = DynamicImage::ImageRgba8(
            RgbaImage::from_vec(
                width,
                height,
                pixels.into_iter().flat_map(|pixel| pixel.0).collect(),
            )
            .unwrap(),
        );
        let mut encoded = std::io::Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        encoded.into_inner()
    }

    // Production mutation caught: accepting transparency or any non-cover RGB value would allow a
    // sampled page pixel to expose content while a response-stage pause remains held.
    #[test]
    fn cover_screenshot_assertion_rejects_one_non_cover_pixel() {
        let covered = screenshot_png(2, vec![Rgba([17, 19, 24, 255]); 4]);
        assert_eq!(assert_cover_screenshot(&covered).unwrap(), 4);

        let flashed = screenshot_png(
            2,
            vec![
                Rgba([17, 19, 24, 255]),
                Rgba([17, 19, 24, 255]),
                Rgba([17, 19, 24, 254]),
                Rgba([17, 19, 24, 255]),
            ],
        );
        assert_eq!(
            assert_cover_screenshot(&flashed).unwrap_err().to_string(),
            "held screenshot pixel at (0, 1) is [17, 19, 24, 254], expected [17, 19, 24, 255]"
        );
    }

    // Production mutation caught: checking only image load completion, rather than rendered
    // screenshot colors, would accept a missing safe tile or leaked original flagged tile.
    #[test]
    fn reveal_screenshot_assertion_requires_safe_and_placeholder_colors_without_flagged_color() {
        let safe_reveal = screenshot_png(
            3,
            vec![
                Rgba([0, 0, 0, 255]),
                Rgba([255, 0, 255, 255]),
                Rgba([2, 0, 0, 255]),
            ],
        );
        let evidence = assert_reveal_screenshot(&safe_reveal, 3, 1).unwrap();
        assert_eq!(evidence.sampled_pixels, 3);
        assert_eq!(evidence.safe_fixture_colors_present, 2);
        assert!(evidence.placeholder_color_present);
        assert!(evidence.original_flagged_color_absent);

        let leaked = screenshot_png(
            4,
            vec![
                Rgba([0, 0, 0, 255]),
                Rgba([255, 0, 255, 255]),
                Rgba([2, 0, 0, 255]),
                Rgba([1, 0, 0, 255]),
            ],
        );
        assert_eq!(
            assert_reveal_screenshot(&leaked, 3, 1)
                .unwrap_err()
                .to_string(),
            "revealed screenshot contains original flagged color [1, 0, 0, 255]"
        );
    }

    // Production mutation caught: merely serializing DOM pixels would let a wrong safe image or
    // wrong replacement reach the summary without making the headed experiment fail.
    #[test]
    fn dom_image_assertion_enforces_every_safe_and_replaced_pixel() {
        let mut images = vec![
            DomImageMetadata {
                index: 0,
                natural_width: 1,
                natural_height: 1,
                rgba: [0, 0, 0, 255],
            },
            DomImageMetadata {
                index: 1,
                natural_width: 1,
                natural_height: 1,
                rgba: [255, 0, 255, 255],
            },
            DomImageMetadata {
                index: 2,
                natural_width: 1,
                natural_height: 1,
                rgba: [2, 0, 0, 255],
            },
        ];
        assert_dom_image_colors(&images, 1).unwrap();

        images[1].rgba = [1, 0, 0, 255];
        assert_eq!(
            assert_dom_image_colors(&images, 1).unwrap_err().to_string(),
            "DOM image 1 rendered [1, 0, 0, 255], expected [255, 0, 255, 255]"
        );
    }

    // Production mutation caught: rounding before enforcing the reveal gate would let a duration
    // just over 500 ms serialize as 500 ms and incorrectly pass.
    #[test]
    fn reveal_latency_gate_accepts_exact_limit_and_rejects_one_nanosecond_over() {
        let settled_at = Instant::now();
        let at_limit =
            ValidatedRevealLatency::between(settled_at, settled_at + Duration::from_millis(500))
                .unwrap();
        assert_eq!(at_limit.summary_millis(), 500);

        assert_eq!(
            ValidatedRevealLatency::between(
                settled_at,
                settled_at + Duration::from_millis(500) + Duration::from_nanos(1),
            )
            .unwrap_err()
            .to_string(),
            "reveal latency of 500000001ns exceeds the 500000000ns limit"
        );
    }

    // Production mutation caught: taking the completion timestamp after revealed-frame capture or
    // PNG decoding would charge later assertion work to response-to-readiness latency.
    #[test]
    fn validated_reveal_latency_excludes_later_screenshot_and_decode_work() {
        let settled_at = Instant::now();
        let readiness_applied_at = settled_at + Duration::from_millis(125);
        let latency = ValidatedRevealLatency::between(settled_at, readiness_applied_at).unwrap();
        let screenshot_decoded_at = readiness_applied_at + Duration::from_secs(9);

        assert_eq!(latency.summary_millis(), 125);
        assert_eq!(
            screenshot_decoded_at.duration_since(settled_at),
            Duration::from_millis(9_125)
        );
    }

    // Production mutations caught: stamping after counter/ledger/metric bookkeeping would omit
    // that work, while stamping after screenshot validation would include work after readiness.
    #[tokio::test]
    async fn production_reveal_timeline_counts_bookkeeping_but_excludes_screenshot_work() {
        let mut counts = RunCounts::default();
        let mut buffer = MetricBuffer::default();
        let mut ledger = PauseLedger::default();
        let request_id = RequestId::new("integrated-reveal-timeline");
        ledger.begin(&request_id).unwrap();

        let outcome = finish_response_after_cdp(true, || {
            std::thread::sleep(Duration::from_millis(80));
            counts.continued += 1;
            resolve_and_buffer_metrics(
                &mut ledger,
                &request_id,
                &mut buffer,
                vec![metric_record(1)],
            )
        })
        .unwrap();

        let latency = finish_reveal_after_response(
            outcome.settled_at,
            async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            },
            |()| async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let screenshot = screenshot_png(
                    3,
                    vec![
                        Rgba([0, 0, 0, 255]),
                        Rgba([255, 0, 255, 255]),
                        Rgba([2, 0, 0, 255]),
                    ],
                );
                let evidence = assert_reveal_screenshot(&screenshot, 3, 1)?;
                assert_eq!(evidence.sampled_pixels, 3);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert!(
            latency.0 >= Duration::from_millis(90),
            "bookkeeping was excluded from {:?}",
            latency.0
        );
        assert!(
            latency.0 < Duration::from_millis(200),
            "post-readiness screenshot work was included in {:?}",
            latency.0
        );
        assert_eq!(counts.continued, 1);
        assert_eq!(ledger.unresolved_count(), 0);
        assert_eq!(buffer.records.len(), 1);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("intentional telemetry failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    enum ScriptedOutcome {
        Success,
        Failure(&'static str),
        Pending,
    }

    async fn scripted_result(outcome: ScriptedOutcome) -> anyhow::Result<()> {
        match outcome {
            ScriptedOutcome::Success => Ok(()),
            ScriptedOutcome::Failure(message) => anyhow::bail!(message),
            ScriptedOutcome::Pending => std::future::pending().await,
        }
    }

    struct ScriptedCleanup {
        close: ScriptedOutcome,
        kill: ScriptedOutcome,
        wait: ScriptedOutcome,
        handler: ScriptedOutcome,
        aborted_handler: ScriptedOutcome,
        calls: Vec<&'static str>,
    }

    impl ScriptedCleanup {
        fn new(
            close: ScriptedOutcome,
            kill: ScriptedOutcome,
            wait: ScriptedOutcome,
            handler: ScriptedOutcome,
        ) -> Self {
            Self {
                close,
                kill,
                wait,
                handler,
                aborted_handler: ScriptedOutcome::Success,
                calls: Vec::new(),
            }
        }
    }

    impl CleanupOperations for ScriptedCleanup {
        async fn close(&mut self) -> anyhow::Result<()> {
            self.calls.push("close");
            scripted_result(self.close).await
        }

        async fn kill(&mut self) -> anyhow::Result<()> {
            self.calls.push("kill");
            scripted_result(self.kill).await
        }

        async fn wait(&mut self) -> anyhow::Result<()> {
            self.calls.push("wait");
            scripted_result(self.wait).await
        }

        async fn join_handler(&mut self) -> anyhow::Result<()> {
            self.calls.push("join_handler");
            scripted_result(self.handler).await
        }

        fn abort_handler(&mut self) {
            self.calls.push("abort_handler");
        }

        async fn reap_aborted_handler(&mut self) -> anyhow::Result<()> {
            self.calls.push("reap_aborted_handler");
            scripted_result(self.aborted_handler).await
        }
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
            "fixture URL must use the controlled http://127.0.0.1 origin"
        );
    }

    // Production mutation caught: accepting IPv6 loopback would widen the browser boundary
    // beyond the fixture-only policy's controlled 127.0.0.1 origin.
    #[test]
    fn rejects_ipv6_loopback_fixture_urls() {
        let root = tempdir().unwrap();
        let chromium = existing_file(root.path(), "chromium");
        let profile = root.path().join("profile");
        let extension = root.path().join("extension");
        fs::create_dir(&profile).unwrap();
        fs::create_dir(&extension).unwrap();

        let error = ExperimentConfig::new(
            Url::parse("http://[::1]:4000/").unwrap(),
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
            "fixture URL must use the controlled http://127.0.0.1 origin"
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
            ledger.begin(&request_id).unwrap_err().to_string(),
            "request pause-1 was observed more than once"
        );
        assert_eq!(ledger.unresolved_count(), 0);
        assert_eq!(
            ledger.resolved(&request_id).unwrap_err().to_string(),
            "request pause-1 was resolved more than once"
        );
    }

    // Production mutation caught: buffering telemetry before ledger resolution could strand a
    // CDP pause when the fixed telemetry capacity is exhausted.
    #[test]
    fn telemetry_capacity_failure_cannot_leave_a_settled_pause_unresolved() {
        let mut buffer = MetricBuffer::default();
        buffer
            .record((0..200).map(metric_record).collect())
            .unwrap();
        let mut ledger = PauseLedger::default();
        let request_id = RequestId::new("pause-telemetry-capacity");
        ledger.begin(&request_id).unwrap();

        let error = resolve_and_buffer_metrics(
            &mut ledger,
            &request_id,
            &mut buffer,
            vec![metric_record(200)],
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "response metrics exceed the 200-record bound"
        );
        assert_eq!(ledger.unresolved_count(), 0);
    }

    // Production mutation caught: moving arbitrary writer I/O back onto the interception path
    // could let a telemetry error prevent an already-resolved response from reaching cleanup.
    #[test]
    fn writer_failure_happens_only_after_pause_resolution() {
        let mut buffer = MetricBuffer::default();
        let mut ledger = PauseLedger::default();
        let request_id = RequestId::new("pause-telemetry-writer");
        ledger.begin(&request_id).unwrap();
        resolve_and_buffer_metrics(
            &mut ledger,
            &request_id,
            &mut buffer,
            vec![metric_record(1)],
        )
        .unwrap();

        let error = flush_metrics(buffer, &mut MetricSink::new(FailingWriter)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "failed to write buffered privacy-safe response metric"
        );
        assert_eq!(ledger.unresolved_count(), 0);
    }

    // Production mutation caught: treating a successful kill fallback as graceful shutdown would
    // hide the failed close that forced Chromium termination.
    #[tokio::test]
    async fn forced_reap_is_never_classified_as_clean_shutdown() {
        let mut cleanup = ScriptedCleanup::new(
            ScriptedOutcome::Failure("close broke"),
            ScriptedOutcome::Success,
            ScriptedOutcome::Success,
            ScriptedOutcome::Success,
        );

        let report = perform_cleanup(&mut cleanup, Duration::from_millis(1)).await;

        assert!(report.forced_reap());
        assert!(!report.is_clean());
        assert_eq!(
            report.into_result().unwrap_err().to_string(),
            "failed to close Chromium gracefully: close broke"
        );
        assert_eq!(cleanup.calls, ["close", "kill", "wait", "join_handler"]);
    }

    // Production mutation caught: returning early on a kill error or timeout would skip bounded
    // process wait and handler termination attempts.
    #[tokio::test]
    async fn kill_error_and_timeout_still_attempt_wait_and_handler_termination() {
        for (kill, expected) in [
            (
                ScriptedOutcome::Failure("kill broke"),
                "failed to kill Chromium after close failure: kill broke",
            ),
            (ScriptedOutcome::Pending, "Chromium kill deadline elapsed"),
        ] {
            let mut cleanup = ScriptedCleanup::new(
                ScriptedOutcome::Failure("close broke"),
                kill,
                ScriptedOutcome::Success,
                ScriptedOutcome::Success,
            );

            let report = perform_cleanup(&mut cleanup, Duration::from_millis(1)).await;
            let error = report.into_result().unwrap_err().to_string();

            assert!(error.contains(expected), "{error}");
            assert_eq!(cleanup.calls, ["close", "kill", "wait", "join_handler"]);
        }
    }

    // Production mutation caught: returning early on a process wait error or timeout would skip
    // the final bounded Chromiumoxide handler termination attempt.
    #[tokio::test]
    async fn wait_error_and_timeout_still_attempt_handler_termination() {
        for (wait, expected) in [
            (
                ScriptedOutcome::Failure("wait broke"),
                "failed to wait for Chromium: wait broke",
            ),
            (ScriptedOutcome::Pending, "Chromium wait deadline elapsed"),
        ] {
            let mut cleanup = ScriptedCleanup::new(
                ScriptedOutcome::Success,
                ScriptedOutcome::Success,
                wait,
                ScriptedOutcome::Success,
            );

            let report = perform_cleanup(&mut cleanup, Duration::from_millis(1)).await;
            let error = report.into_result().unwrap_err().to_string();

            assert!(error.contains(expected), "{error}");
            assert_eq!(cleanup.calls, ["close", "wait", "join_handler"]);
        }
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
