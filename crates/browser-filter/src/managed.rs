use std::{collections::HashSet, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    cdp::browser_protocol::{
        fetch::{EventRequestPaused, GetResponseBodyParams, RequestId},
        page::{
            EventFrameNavigated, EventLifecycleEvent, GetFrameTreeParams, NavigateParams,
            SetLifecycleEventsEnabledParams,
        },
        target::{CloseTargetParams, EventTargetCreated, EventTargetDestroyed, TargetId},
    },
};
use futures::StreamExt;
use serde::Serialize;
use url::Url;

use crate::{
    browser::{
        HeadedDetectorSession, cleanup_browser, continue_response, decode_response_body,
        fetch_enable_params, replacement_response,
    },
    inference::{DEFAULT_MAX_ENCODED_BYTES, Detector, InferenceReport},
    policy::{Policy, Verdict},
};

const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct ManagedBrowserConfig {
    start_url: Option<Url>,
    chromium_bin: PathBuf,
    profile_dir: PathBuf,
    extension_dir: PathBuf,
    acquisition_timeout: Duration,
    inference_timeout: Duration,
    lifecycle_timeout: Duration,
}

impl ManagedBrowserConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start_url: Option<Url>,
        chromium_bin: PathBuf,
        profile_dir: PathBuf,
        extension_dir: PathBuf,
        acquisition_timeout: Duration,
        inference_timeout: Duration,
        lifecycle_timeout: Duration,
    ) -> Result<Self> {
        if let Some(url) = &start_url {
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https"),
                "start URL must use http or https"
            );
        }
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
            [acquisition_timeout, inference_timeout, lifecycle_timeout]
                .into_iter()
                .all(|timeout| !timeout.is_zero() && timeout <= MAX_OPERATION_TIMEOUT),
            "timeouts must be within 1ns..=30s"
        );
        Ok(Self {
            start_url,
            chromium_bin,
            profile_dir,
            extension_dir,
            acquisition_timeout,
            inference_timeout,
            lifecycle_timeout,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponsePlan {
    ContinueRedirect,
    Classify,
    ReplaceFailedClosed,
}

fn response_plan(response_status_code: Option<i64>, has_response_error: bool) -> ResponsePlan {
    if has_response_error {
        return ResponsePlan::ReplaceFailedClosed;
    }
    match response_status_code {
        Some(200) => ResponsePlan::Classify,
        Some(301 | 302 | 303 | 307 | 308) => ResponsePlan::ContinueRedirect,
        _ => ResponsePlan::ReplaceFailedClosed,
    }
}

fn supports_complete_frame_analysis(encoded: &[u8]) -> bool {
    match image::guess_format(encoded) {
        Ok(image::ImageFormat::Jpeg) => true,
        Ok(image::ImageFormat::Png) => !encoded.windows(4).any(|window| window == b"acTL"),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    Allow,
    ReplaceExplicit,
    ReplaceFailedClosed,
}

fn classification_for(
    policy: &Policy,
    request_url: &Url,
    report: std::result::Result<&InferenceReport, ()>,
) -> Classification {
    match report {
        Ok(report) => match policy.decide(request_url, report) {
            Verdict::Allow => Classification::Allow,
            Verdict::Replace { .. } => Classification::ReplaceExplicit,
        },
        Err(_) => Classification::ReplaceFailedClosed,
    }
}

fn should_reveal(load_seen: bool, unresolved_responses: usize) -> bool {
    load_seen && unresolved_responses == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetAction {
    Keep,
    Close,
    Ignore,
}

fn target_action(primary_id: &str, target_id: &str, target_type: &str) -> TargetAction {
    match (target_type, target_id == primary_id) {
        ("page", true) => TargetAction::Keep,
        ("page", false) => TargetAction::Close,
        _ => TargetAction::Ignore,
    }
}

fn reveal_failure_is_stale(active_loader: Option<&str>, before: &str, after: &str) -> bool {
    before != after || active_loader != Some(after)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ManualCounts {
    intercepted: usize,
    continued: usize,
    replaced: usize,
    failed_closed: usize,
}

#[derive(Default)]
struct ManualPauseLedger {
    seen: HashSet<RequestId>,
    unresolved: HashSet<RequestId>,
}

impl ManualPauseLedger {
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
fn settle_decision(
    request_id: &RequestId,
    classification: Classification,
    ledger: &mut ManualPauseLedger,
    counts: &mut ManualCounts,
    resolve: impl FnOnce(bool) -> Result<()>,
) -> Result<()> {
    let replace = classification != Classification::Allow;
    resolve(replace)?;
    ledger.resolved(request_id)?;
    counts.intercepted += 1;
    if replace {
        counts.replaced += 1;
    } else {
        counts.continued += 1;
    }
    if classification == Classification::ReplaceFailedClosed {
        counts.failed_closed += 1;
    }
    Ok(())
}

#[derive(Default)]
struct ManualPageState {
    active_loader: Option<String>,
    loaded_loader: Option<String>,
    ledger: ManualPauseLedger,
    counts: ManualCounts,
}

impl ManualPageState {
    fn navigation_started(&mut self, loader_id: &str) {
        self.active_loader = Some(loader_id.to_owned());
        self.loaded_loader = None;
    }

    fn begin_response(&mut self, request_id: &RequestId) -> Result<()> {
        self.ledger.begin(request_id)
    }

    fn load_completed(&mut self, loader_id: &str) -> bool {
        if self.active_loader.as_deref() == Some(loader_id) {
            self.loaded_loader = Some(loader_id.to_owned());
        }
        self.ready_to_reveal()
    }

    fn ready_to_reveal(&self) -> bool {
        should_reveal(
            self.active_loader.is_some() && self.active_loader == self.loaded_loader,
            self.ledger.unresolved_count(),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct ManagedBrowserSummary {
    pub chromium_version: String,
    pub onnx_runtime_version: String,
    pub model_sha256: String,
    pub intercepted: usize,
    pub continued: usize,
    pub replaced: usize,
    pub failed_closed: usize,
    pub blocked_extra_pages: usize,
    pub unresolved: usize,
    pub clean_shutdown: bool,
}

pub struct ManagedBrowser {
    detector: Detector,
    policy: Policy,
}

impl ManagedBrowser {
    pub fn new(detector: Detector, policy: Policy) -> Self {
        Self { detector, policy }
    }

    pub fn run(self, config: ManagedBrowserConfig) -> Result<ManagedBrowserSummary> {
        let runtime_shutdown_timeout = config.inference_timeout;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create managed-browser runtime")?;
        let result = runtime.block_on(run_managed_browser(self, config));
        runtime.shutdown_timeout(runtime_shutdown_timeout);
        result
    }
}

async fn run_managed_browser(
    managed: ManagedBrowser,
    config: ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    let ManagedBrowser { detector, policy } = managed;
    let browser_config = BrowserConfig::builder()
        .chrome_executable(&config.chromium_bin)
        .with_head()
        .user_data_dir(&config.profile_dir)
        .extension(config.extension_dir.display().to_string())
        .window_size(1280, 800)
        .launch_timeout(config.lifecycle_timeout)
        .request_timeout(config.acquisition_timeout)
        .disable_cache()
        .build()
        .map_err(anyhow::Error::msg)?;
    let detector_session = HeadedDetectorSession::start(detector);

    let browser_result = match Browser::launch(browser_config).await {
        Ok((browser, handler)) => {
            run_managed_with_browser(browser, handler, &detector_session, &policy, &config).await
        }
        Err(error) => Err(error).context("failed to launch managed Chromium"),
    };
    let worker_result = detector_session.shutdown(config.inference_timeout).await;

    match (browser_result, worker_result) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("managed inference worker cleanup failed"),
        (Err(browser_error), Err(worker_error)) => Err(anyhow::anyhow!(
            "{browser_error:#}; managed inference worker cleanup failed: {worker_error:#}"
        )),
    }
}

async fn run_managed_with_browser(
    mut browser: Browser,
    mut handler: chromiumoxide::Handler,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    config: &ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            event.context("Chromiumoxide handler failed")?;
        }
        Ok::<_, anyhow::Error>(())
    });

    let run_result = managed_event_loop(&mut browser, detector_session, policy, config).await;
    let already_exited = browser
        .try_wait()
        .context("failed to inspect managed Chromium process")?
        .is_some();
    let cleanup_result = if already_exited {
        finish_exited_browser_handler(handler_task, config.lifecycle_timeout).await
    } else {
        cleanup_browser(&mut browser, handler_task, config.lifecycle_timeout)
            .await
            .into_result()
    };

    match (run_result, cleanup_result) {
        (Ok(mut summary), Ok(())) => {
            summary.clean_shutdown = true;
            anyhow::ensure!(
                summary.unresolved == 0,
                "{} response pauses remain unresolved",
                summary.unresolved
            );
            Ok(summary)
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("managed browser cleanup failed"),
        (Err(run_error), Err(cleanup_error)) => Err(anyhow::anyhow!(
            "{run_error:#}; managed browser cleanup failed: {cleanup_error:#}"
        )),
    }
}

async fn finish_exited_browser_handler(
    mut handler_task: tokio::task::JoinHandle<Result<()>>,
    deadline: Duration,
) -> Result<()> {
    match tokio::time::timeout(deadline, &mut handler_task).await {
        Ok(joined) => joined.context("managed Chromium handler task failed")?,
        Err(_) => {
            handler_task.abort();
            match tokio::time::timeout(deadline, handler_task).await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) if error.is_cancelled() => Ok(()),
                Ok(Err(error)) => {
                    Err(error).context("aborted managed Chromium handler task failed")
                }
                Err(_) => anyhow::bail!("aborted managed Chromium handler reap deadline elapsed"),
            }
        }
    }
}

async fn managed_event_loop(
    browser: &mut Browser,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    config: &ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    let page = within(
        config.lifecycle_timeout,
        browser.new_page("about:blank"),
        "about:blank page acquisition deadline elapsed",
    )
    .await?
    .context("failed to create managed about:blank page")?;
    let primary_id = page.target_id().clone();

    let mut blocked_extra_pages =
        close_startup_extra_pages(browser, &primary_id, config.acquisition_timeout).await?;
    let chromium_version = within(
        config.acquisition_timeout,
        browser.version(),
        "Chromium version acquisition deadline elapsed",
    )
    .await?
    .context("failed to read Chromium version")?
    .product;
    let mut pauses = within(
        config.acquisition_timeout,
        page.event_listener::<EventRequestPaused>(),
        "Fetch listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Fetch.requestPaused listener")?;
    let mut frames = within(
        config.acquisition_timeout,
        page.event_listener::<EventFrameNavigated>(),
        "frame listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Page.frameNavigated listener")?;
    let mut lifecycle = within(
        config.acquisition_timeout,
        page.event_listener::<EventLifecycleEvent>(),
        "lifecycle listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Page.lifecycleEvent listener")?;
    let mut targets = within(
        config.acquisition_timeout,
        browser.event_listener::<EventTargetCreated>(),
        "target listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Target.targetCreated listener")?;
    let mut destroyed = within(
        config.acquisition_timeout,
        browser.event_listener::<EventTargetDestroyed>(),
        "target-destroyed listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Target.targetDestroyed listener")?;

    within(
        config.acquisition_timeout,
        page.execute(SetLifecycleEventsEnabledParams::new(true)),
        "Page.setLifecycleEventsEnabled deadline elapsed",
    )
    .await?
    .context("failed to enable page lifecycle events")?;
    within(
        config.acquisition_timeout,
        page.execute(fetch_enable_params()),
        "Fetch.enable deadline elapsed",
    )
    .await?
    .context("failed to enable image response interception")?;

    let mut state = ManualPageState::default();
    let initial = current_main_frame(&page, config.acquisition_timeout).await?;
    state.navigation_started(initial.1.as_ref());
    state.load_completed(initial.1.as_ref());
    reveal_current_document(&page, config.acquisition_timeout).await?;
    within(
        config.acquisition_timeout,
        page.activate(),
        "managed page activation deadline elapsed",
    )
    .await?
    .context("failed to activate managed page")?;

    let initial_url = config.start_url.as_ref().map(Url::as_str);
    let mut initial_navigation = Box::pin(async {
        if let Some(url) = initial_url {
            within(
                config.lifecycle_timeout,
                page.execute(NavigateParams::new(url)),
                "initial navigation deadline elapsed",
            )
            .await?
            .context("failed to complete initial navigation")?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let mut initial_navigation_complete = initial_url.is_none();

    let mut exit_poll = tokio::time::interval(Duration::from_millis(250));
    exit_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            result = &mut initial_navigation, if !initial_navigation_complete => {
                result?;
                initial_navigation_complete = true;
            }
            event = pauses.next() => {
                let Some(event) = event else { break; };
                process_managed_pause(&page, event.as_ref(), detector_session, policy, config, &mut state).await?;
                reveal_if_current_and_ready(&page, config, &state).await?;
            }
            event = frames.next() => {
                let Some(event) = event else { break; };
                if event.frame.parent_id.is_none() {
                    state.navigation_started(event.frame.loader_id.as_ref());
                }
            }
            event = lifecycle.next() => {
                let Some(event) = event else { break; };
                if event.name == "load" && state.load_completed(event.loader_id.as_ref()) {
                    reveal_if_current_and_ready(&page, config, &state).await?;
                }
            }
            event = targets.next() => {
                let Some(event) = browser_stream_event(event, browser, "Target.targetCreated")? else { break; };
                match target_action(
                    primary_id.as_ref(),
                    event.target_info.target_id.as_ref(),
                    &event.target_info.r#type,
                ) {
                    TargetAction::Keep | TargetAction::Ignore => {}
                    TargetAction::Close => {
                        close_target(browser, event.target_info.target_id.clone(), config.acquisition_timeout).await?;
                        blocked_extra_pages += 1;
                    }
                }
            }
            event = destroyed.next() => {
                let Some(event) = browser_stream_event(event, browser, "Target.targetDestroyed")? else { break; };
                if event.target_id == primary_id {
                    break;
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for Ctrl-C")?;
                break;
            }
            _ = exit_poll.tick() => {
                if browser.try_wait().context("failed to inspect managed Chromium process")?.is_some() {
                    break;
                }
            }
        }
    }

    let metadata = detector_session.metadata();
    Ok(ManagedBrowserSummary {
        chromium_version,
        onnx_runtime_version: metadata.runtime_version.clone(),
        model_sha256: metadata.model_sha256.clone(),
        intercepted: state.counts.intercepted,
        continued: state.counts.continued,
        replaced: state.counts.replaced,
        failed_closed: state.counts.failed_closed,
        blocked_extra_pages,
        unresolved: state.ledger.unresolved_count(),
        clean_shutdown: false,
    })
}

async fn within<F, T>(deadline: Duration, future: F, message: &'static str) -> Result<T>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(deadline, future)
        .await
        .context(message)
}

fn browser_stream_event<T>(
    event: Option<std::sync::Arc<T>>,
    browser: &mut Browser,
    name: &str,
) -> Result<Option<std::sync::Arc<T>>> {
    match event {
        Some(event) => Ok(Some(event)),
        None if browser
            .try_wait()
            .context("failed to inspect managed Chromium process")?
            .is_some() =>
        {
            Ok(None)
        }
        None => anyhow::bail!("{name} stream ended while managed Chromium was active"),
    }
}

async fn close_startup_extra_pages(
    browser: &Browser,
    primary_id: &TargetId,
    deadline: Duration,
) -> Result<usize> {
    let pages = within(deadline, browser.pages(), "page inventory deadline elapsed")
        .await?
        .context("failed to inventory managed browser pages")?;
    let mut closed = 0;
    for page in pages {
        if page.target_id() != primary_id {
            close_target(browser, page.target_id().clone(), deadline).await?;
            closed += 1;
        }
    }
    Ok(closed)
}

async fn close_target(browser: &Browser, target_id: TargetId, deadline: Duration) -> Result<()> {
    within(
        deadline,
        browser.execute(CloseTargetParams::new(target_id)),
        "additional page closure deadline elapsed",
    )
    .await?
    .context("failed to close an additional page")?;
    Ok(())
}

async fn current_main_frame(
    page: &Page,
    deadline: Duration,
) -> Result<(
    chromiumoxide::cdp::browser_protocol::page::FrameId,
    chromiumoxide::cdp::browser_protocol::network::LoaderId,
)> {
    let tree = within(
        deadline,
        page.execute(GetFrameTreeParams::default()),
        "frame-tree acquisition deadline elapsed",
    )
    .await?
    .context("failed to acquire current frame tree")?
    .result
    .frame_tree;
    Ok((tree.frame.id, tree.frame.loader_id))
}

async fn reveal_if_current_and_ready(
    page: &Page,
    config: &ManagedBrowserConfig,
    state: &ManualPageState,
) -> Result<()> {
    if !state.ready_to_reveal() {
        return Ok(());
    }
    let (_, current_loader) = current_main_frame(page, config.acquisition_timeout).await?;
    if state.active_loader.as_deref() == Some(current_loader.as_ref())
        && let Err(error) = reveal_current_document(page, config.acquisition_timeout).await
    {
        let (_, after_loader) = current_main_frame(page, config.acquisition_timeout).await?;
        if !reveal_failure_is_stale(
            state.active_loader.as_deref(),
            current_loader.as_ref(),
            after_loader.as_ref(),
        ) {
            return Err(error);
        }
    }
    Ok(())
}

async fn reveal_current_document(page: &Page, deadline: Duration) -> Result<()> {
    let result = within(
        deadline,
        page.evaluate(
            r#"(() => {
                document.documentElement.setAttribute('data-omarchy-kids-ready', '');
                return document.documentElement.hasAttribute('data-omarchy-kids-ready');
            })()"#,
        ),
        "managed page reveal deadline elapsed",
    )
    .await?
    .context("failed to set managed-page readiness")?;
    let ready = result
        .into_value::<bool>()
        .context("managed-page readiness result had an unexpected shape")?;
    anyhow::ensure!(ready, "managed-page readiness attribute was not set");
    Ok(())
}

async fn process_managed_pause(
    page: &Page,
    event: &EventRequestPaused,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    config: &ManagedBrowserConfig,
    state: &mut ManualPageState,
) -> Result<()> {
    state.begin_response(&event.request_id)?;
    let classification =
        prepare_managed_classification(page, event, detector_session, policy, config).await;
    let replace = classification != Classification::Allow;
    if replace {
        within(
            config.acquisition_timeout,
            page.execute(replacement_response(event.request_id.clone())),
            "Fetch.fulfillRequest deadline elapsed",
        )
        .await?
        .context("failed to fulfill a managed image response")?;
    } else {
        within(
            config.acquisition_timeout,
            page.execute(continue_response(event.request_id.clone())),
            "Fetch.continueResponse deadline elapsed",
        )
        .await?
        .context("failed to continue a managed image response")?;
    }
    state.ledger.resolved(&event.request_id)?;
    state.counts.intercepted += 1;
    if replace {
        state.counts.replaced += 1;
    } else {
        state.counts.continued += 1;
    }
    if classification == Classification::ReplaceFailedClosed {
        state.counts.failed_closed += 1;
    }
    Ok(())
}

async fn prepare_managed_classification(
    page: &Page,
    event: &EventRequestPaused,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    config: &ManagedBrowserConfig,
) -> Classification {
    match response_plan(
        event.response_status_code,
        event.response_error_reason.is_some(),
    ) {
        ResponsePlan::ContinueRedirect => Classification::Allow,
        ResponsePlan::ReplaceFailedClosed => Classification::ReplaceFailedClosed,
        ResponsePlan::Classify => {
            let result = async {
                let request_url = Url::parse(&event.request.url)
                    .context("managed image response URL was invalid")?;
                let body = within(
                    config.acquisition_timeout,
                    page.execute(GetResponseBodyParams::new(event.request_id.clone())),
                    "Fetch.getResponseBody deadline elapsed",
                )
                .await?
                .context("failed to acquire managed image response body")?
                .result;
                let encoded = decode_response_body(
                    &body.body,
                    body.base64_encoded,
                    DEFAULT_MAX_ENCODED_BYTES,
                )?;
                anyhow::ensure!(
                    supports_complete_frame_analysis(&encoded),
                    "managed image format cannot be completely analyzed"
                );
                let report = detector_session
                    .detect(encoded, config.inference_timeout)
                    .await?;
                Ok::<_, anyhow::Error>(classification_for(policy, &request_url, Ok(&report)))
            }
            .await;
            result.unwrap_or(Classification::ReplaceFailedClosed)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use chromiumoxide::cdp::browser_protocol::fetch::RequestId;
    use tempfile::TempDir;
    use url::Url;

    use crate::{
        inference::{Detection, InferenceReport},
        policy::Policy,
    };

    use super::{
        Classification, ManagedBrowserConfig, ManagedBrowserSummary, ManualCounts, ManualPageState,
        ManualPauseLedger, ResponsePlan, TargetAction, classification_for, response_plan,
        reveal_failure_is_stale, settle_decision, should_reveal, supports_complete_frame_analysis,
        target_action,
    };

    struct ConfigFixture {
        root: TempDir,
        chromium: PathBuf,
        profile: PathBuf,
        extension: PathBuf,
    }

    impl ConfigFixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let chromium = root.path().join("chromium");
            fs::write(&chromium, b"binary").unwrap();
            let profile = root.path().join("profile");
            fs::create_dir(&profile).unwrap();
            let extension = root.path().join("extension");
            fs::create_dir(&extension).unwrap();
            fs::write(extension.join("manifest.json"), b"{}").unwrap();
            Self {
                root,
                chromium,
                profile,
                extension,
            }
        }

        fn build(&self, start_url: Option<Url>) -> anyhow::Result<ManagedBrowserConfig> {
            ManagedBrowserConfig::new(
                start_url,
                self.chromium.clone(),
                self.profile.clone(),
                self.extension.clone(),
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(30),
            )
        }
    }

    #[test]
    fn config_accepts_blank_http_and_https_start_pages() {
        for start_url in [
            None,
            Some(Url::parse("http://example.test/").unwrap()),
            Some(Url::parse("https://example.test/").unwrap()),
        ] {
            ConfigFixture::new().build(start_url).unwrap();
        }
    }

    #[test]
    fn config_rejects_non_web_start_pages() {
        let fixture = ConfigFixture::new();
        let error = fixture
            .build(Some(Url::parse("file:///etc/passwd").unwrap()))
            .unwrap_err();
        assert_eq!(error.to_string(), "start URL must use http or https");
    }

    #[test]
    fn config_rejects_missing_browser_or_extension_manifest_and_reused_profile() {
        let fixture = ConfigFixture::new();
        fs::remove_file(&fixture.chromium).unwrap();
        assert_eq!(
            fixture.build(None).unwrap_err().to_string(),
            "Chromium executable does not exist"
        );

        let fixture = ConfigFixture::new();
        fs::remove_file(fixture.extension.join("manifest.json")).unwrap();
        assert_eq!(
            fixture.build(None).unwrap_err().to_string(),
            "extension manifest does not exist"
        );

        let fixture = ConfigFixture::new();
        fs::write(fixture.profile.join("History"), b"prior state").unwrap();
        assert_eq!(
            fixture.build(None).unwrap_err().to_string(),
            "Chromium profile must be a new empty directory"
        );
    }

    #[test]
    fn config_rejects_zero_and_over_thirty_second_timeouts() {
        for timeouts in [
            [
                Duration::ZERO,
                Duration::from_secs(5),
                Duration::from_secs(30),
            ],
            [
                Duration::from_secs(5),
                Duration::ZERO,
                Duration::from_secs(30),
            ],
            [
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::ZERO,
            ],
            [
                Duration::from_secs(31),
                Duration::from_secs(5),
                Duration::from_secs(30),
            ],
        ] {
            let fixture = ConfigFixture::new();
            let error = ManagedBrowserConfig::new(
                None,
                fixture.chromium.clone(),
                fixture.profile.clone(),
                fixture.extension.clone(),
                timeouts[0],
                timeouts[1],
                timeouts[2],
            )
            .unwrap_err();
            assert_eq!(error.to_string(), "timeouts must be within 1ns..=30s");
        }
    }

    #[test]
    fn config_rejects_default_chromium_profiles() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        for browser in ["chromium", "google-chrome"] {
            let fixture = ConfigFixture::new();
            let error = ManagedBrowserConfig::new(
                None,
                fixture.chromium.clone(),
                PathBuf::from(&home).join(".config").join(browser),
                fixture.extension.clone(),
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
    }

    #[test]
    fn fixture_keeps_its_temporary_root_alive() {
        let fixture = ConfigFixture::new();
        assert!(fixture.root.path().is_dir());
    }

    fn report(detections: Vec<Detection>) -> InferenceReport {
        InferenceReport {
            detections,
            encoded_bytes: 70,
            width: 1,
            height: 1,
            decode_micros: 2,
            preprocess_micros: 3,
            inference_micros: 5,
            postprocess_micros: 7,
        }
    }

    #[test]
    fn response_plan_classifies_only_successful_bodies_and_continues_only_redirects() {
        assert_eq!(response_plan(Some(200), false), ResponsePlan::Classify);
        for status in [301, 302, 303, 307, 308] {
            assert_eq!(
                response_plan(Some(status), false),
                ResponsePlan::ContinueRedirect
            );
        }
        for status in [
            None,
            Some(101),
            Some(204),
            Some(206),
            Some(304),
            Some(404),
            Some(500),
        ] {
            assert_eq!(
                response_plan(status, false),
                ResponsePlan::ReplaceFailedClosed
            );
        }
        assert_eq!(
            response_plan(Some(200), true),
            ResponsePlan::ReplaceFailedClosed
        );
    }

    #[test]
    fn classification_allows_only_a_successful_policy_allow() {
        let url = Url::parse("https://example.test/image.png").unwrap();
        let safe = report(Vec::new());
        assert_eq!(
            classification_for(&Policy, &url, Ok(&safe)),
            Classification::Allow
        );

        let explicit = report(vec![Detection {
            class_name: "FEMALE_BREAST_EXPOSED",
            score: 0.91,
            bounding_box: [0, 0, 1, 1],
        }]);
        assert_eq!(
            classification_for(&Policy, &url, Ok(&explicit)),
            Classification::ReplaceExplicit
        );
        assert_eq!(
            classification_for(&Policy, &url, Err(())),
            Classification::ReplaceFailedClosed
        );
    }

    #[test]
    fn manual_mode_accepts_only_static_jpeg_and_png_images() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 0];
        let png = b"\x89PNG\r\n\x1a\nIHDRstatic";
        let apng = b"\x89PNG\r\n\x1a\nIHDRacTLanimated";
        let gif = b"GIF89a";
        let webp = b"RIFF\x10\0\0\0WEBPVP8 ";

        assert!(supports_complete_frame_analysis(&jpeg));
        assert!(supports_complete_frame_analysis(png));
        assert!(!supports_complete_frame_analysis(apng));
        assert!(!supports_complete_frame_analysis(gif));
        assert!(!supports_complete_frame_analysis(webp));
        assert!(!supports_complete_frame_analysis(b"not-an-image"));
    }

    #[test]
    fn reveal_requires_a_load_event_and_zero_unresolved_responses() {
        assert!(!should_reveal(false, 0));
        assert!(!should_reveal(true, 1));
        assert!(should_reveal(true, 0));
    }

    #[test]
    fn only_additional_page_targets_are_closed() {
        assert_eq!(
            target_action("primary", "primary", "page"),
            TargetAction::Keep
        );
        assert_eq!(
            target_action("primary", "popup", "page"),
            TargetAction::Close
        );
        assert_eq!(
            target_action("primary", "worker", "service_worker"),
            TargetAction::Ignore
        );
    }

    #[test]
    fn reveal_error_is_ignored_only_when_navigation_made_its_context_stale() {
        assert!(reveal_failure_is_stale(
            Some("loader-new"),
            "loader-old",
            "loader-new"
        ));
        assert!(reveal_failure_is_stale(
            Some("loader-current"),
            "loader-current",
            "loader-next"
        ));
        assert!(!reveal_failure_is_stale(
            Some("loader-current"),
            "loader-current",
            "loader-current"
        ));
    }

    #[test]
    fn summary_schema_contains_only_privacy_safe_identity_and_counts() {
        let summary = ManagedBrowserSummary {
            chromium_version: "Chromium/152".to_owned(),
            onnx_runtime_version: "1.27.1".to_owned(),
            model_sha256: "model-sha".to_owned(),
            intercepted: 4,
            continued: 2,
            replaced: 2,
            failed_closed: 1,
            blocked_extra_pages: 0,
            unresolved: 0,
            clean_shutdown: true,
        };
        let value = serde_json::to_value(summary).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "blocked_extra_pages",
                "chromium_version",
                "clean_shutdown",
                "continued",
                "failed_closed",
                "intercepted",
                "model_sha256",
                "onnx_runtime_version",
                "replaced",
                "unresolved",
            ]
        );
        assert!(!value.to_string().contains("http"));
    }

    fn request_id(value: &str) -> RequestId {
        RequestId::from(value.to_owned())
    }

    #[test]
    fn settlement_continues_only_allowed_images_and_fulfills_both_replacement_classes() {
        for (classification, expected_replace) in [
            (Classification::Allow, false),
            (Classification::ReplaceExplicit, true),
            (Classification::ReplaceFailedClosed, true),
        ] {
            let request = request_id("request");
            let mut ledger = ManualPauseLedger::default();
            let mut counts = ManualCounts::default();
            ledger.begin(&request).unwrap();
            let mut calls = Vec::new();

            settle_decision(
                &request,
                classification,
                &mut ledger,
                &mut counts,
                |replace| {
                    calls.push(replace);
                    Ok(())
                },
            )
            .unwrap();

            assert_eq!(calls, [expected_replace]);
            assert_eq!(ledger.unresolved_count(), 0);
            assert_eq!(counts.intercepted, 1);
            assert_eq!(counts.continued, usize::from(!expected_replace));
            assert_eq!(counts.replaced, usize::from(expected_replace));
            assert_eq!(
                counts.failed_closed,
                usize::from(classification == Classification::ReplaceFailedClosed)
            );
        }
    }

    #[test]
    fn settlement_updates_nothing_until_cdp_resolution_succeeds() {
        let request = request_id("request");
        let mut ledger = ManualPauseLedger::default();
        let mut counts = ManualCounts::default();
        ledger.begin(&request).unwrap();

        let error = settle_decision(
            &request,
            Classification::ReplaceFailedClosed,
            &mut ledger,
            &mut counts,
            |_| anyhow::bail!("CDP broke"),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "CDP broke");
        assert_eq!(ledger.unresolved_count(), 1);
        assert_eq!(counts, ManualCounts::default());
    }

    #[test]
    fn pause_ledger_rejects_duplicate_and_double_resolution() {
        let request = request_id("request");
        let mut ledger = ManualPauseLedger::default();
        ledger.begin(&request).unwrap();
        assert_eq!(
            ledger.begin(&request).unwrap_err().to_string(),
            "request request was observed more than once"
        );
        ledger.resolved(&request).unwrap();
        assert_eq!(
            ledger.resolved(&request).unwrap_err().to_string(),
            "request request was resolved more than once"
        );
    }

    #[test]
    fn page_state_reveals_after_load_and_the_final_response_settles() {
        let mut state = ManualPageState::default();
        let request = request_id("request");
        state.navigation_started("loader-a");
        state.begin_response(&request).unwrap();
        assert!(!state.load_completed("loader-a"));
        settle_decision(
            &request,
            Classification::Allow,
            &mut state.ledger,
            &mut state.counts,
            |_| Ok(()),
        )
        .unwrap();
        assert!(state.ready_to_reveal());
    }

    #[test]
    fn later_navigation_cannot_reuse_an_earlier_load_event() {
        let mut state = ManualPageState::default();
        state.navigation_started("loader-a");
        assert!(state.load_completed("loader-a"));
        state.navigation_started("loader-b");
        let request = request_id("later");
        state.begin_response(&request).unwrap();
        settle_decision(
            &request,
            Classification::Allow,
            &mut state.ledger,
            &mut state.counts,
            |_| Ok(()),
        )
        .unwrap();
        assert!(!state.ready_to_reveal());
        assert!(state.load_completed("loader-b"));
    }

    #[test]
    fn stale_load_event_cannot_reveal_the_current_navigation() {
        let mut state = ManualPageState::default();
        state.navigation_started("loader-current");
        assert!(!state.load_completed("loader-prior"));
        assert!(!state.ready_to_reveal());
        assert!(state.load_completed("loader-current"));
    }
}
