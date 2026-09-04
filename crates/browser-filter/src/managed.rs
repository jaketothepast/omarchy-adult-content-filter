use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    cdp::browser_protocol::{
        browser::{SetDownloadBehaviorBehavior, SetDownloadBehaviorParams},
        fetch::{
            ContinueRequestParams, EventRequestPaused, FailRequestParams, GetResponseBodyParams,
            HeaderEntry, RequestId,
        },
        network::{
            ErrorReason, EventRequestWillBeSent, Headers, LoaderId, RequestId as NetworkRequestId,
        },
        page::{
            AddScriptToEvaluateOnNewDocumentParams, EventFrameNavigated, EventLifecycleEvent,
            GetFrameTreeParams, NavigateParams, SetLifecycleEventsEnabledParams,
        },
        target::{CloseTargetParams, EventTargetCreated, EventTargetDestroyed, TargetId},
    },
    listeners::EventStream,
};
use futures::StreamExt;
use serde::Serialize;
use url::Url;

use crate::{
    browser::{
        HeadedDetectorSession, cleanup_browser, continue_response, decode_response_body,
        managed_fetch_enable_params, replacement_response,
    },
    inference::{DEFAULT_MAX_ENCODED_BYTES, Detector, InferenceReport},
    policy::{Policy, Verdict},
    request_policy::{RequestDecision, RequestPolicy},
};

const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

fn managed_chromium_argument() -> (&'static str, &'static str) {
    ("ozone-platform", "wayland")
}

fn managed_download_behavior() -> SetDownloadBehaviorParams {
    SetDownloadBehaviorParams::new(SetDownloadBehaviorBehavior::Deny)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PauseStage {
    Request,
    Response,
}

fn pause_stage(response_status_code: Option<i64>, has_response_error: bool) -> PauseStage {
    if response_status_code.is_some() || has_response_error {
        PauseStage::Response
    } else {
        PauseStage::Request
    }
}

enum RequestCommand {
    Fail(FailRequestParams),
    Continue(ContinueRequestParams),
}

impl RequestCommand {
    #[cfg(test)]
    fn is_fail(&self) -> bool {
        matches!(self, Self::Fail(_))
    }

    #[cfg(test)]
    fn json(&self) -> serde_json::Value {
        match self {
            Self::Fail(params) => serde_json::to_value(params).unwrap(),
            Self::Continue(params) => serde_json::to_value(params).unwrap(),
        }
    }
}

fn request_command(request_id: RequestId, decision: RequestDecision) -> RequestCommand {
    match decision {
        RequestDecision::BlockDomain => RequestCommand::Fail(FailRequestParams::new(
            request_id,
            ErrorReason::BlockedByClient,
        )),
        RequestDecision::Continue => {
            RequestCommand::Continue(ContinueRequestParams::new(request_id))
        }
        RequestDecision::RewriteUrl(url) => RequestCommand::Continue(
            ContinueRequestParams::builder()
                .request_id(request_id)
                .url(url)
                .build()
                .expect("rewritten request contains its required identifier"),
        ),
        RequestDecision::ReplaceHeaders(headers) => RequestCommand::Continue(
            ContinueRequestParams::builder()
                .request_id(request_id)
                .headers(
                    headers
                        .into_iter()
                        .map(|(name, value)| HeaderEntry::new(name, value)),
                )
                .build()
                .expect("header-hardened request contains its required identifier"),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestLayerOutcome {
    Continued,
    DomainBlocked,
    SafeSearchRewritten,
    YouTubeRestricted,
}

fn request_outcome(decision: &RequestDecision) -> RequestLayerOutcome {
    match decision {
        RequestDecision::BlockDomain => RequestLayerOutcome::DomainBlocked,
        RequestDecision::Continue => RequestLayerOutcome::Continued,
        RequestDecision::RewriteUrl(_) => RequestLayerOutcome::SafeSearchRewritten,
        RequestDecision::ReplaceHeaders(_) => RequestLayerOutcome::YouTubeRestricted,
    }
}

fn network_headers(headers: &Headers) -> Result<Vec<(String, String)>> {
    let object = headers
        .inner()
        .as_object()
        .context("managed request headers were not an object")?;
    object
        .iter()
        .map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.clone(), value.to_owned()))
                .with_context(|| format!("managed request header {name} was not text"))
        })
        .collect()
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
enum NavigationDecision {
    ContinueMainFrame,
    IgnoreSubframe,
    DenyMainFrame,
}

fn navigation_decision(is_subframe: bool, raw_url: &str) -> NavigationDecision {
    if is_subframe {
        return NavigationDecision::IgnoreSubframe;
    }
    if Url::parse(raw_url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
    {
        NavigationDecision::ContinueMainFrame
    } else {
        NavigationDecision::DenyMainFrame
    }
}

fn apply_navigation_decision(
    decision: NavigationDecision,
    loader_id: &str,
    state: &mut ManualPageState,
) -> Result<()> {
    match decision {
        NavigationDecision::ContinueMainFrame => state.navigation_started(loader_id),
        NavigationDecision::IgnoreSubframe => {}
        NavigationDecision::DenyMainFrame => {
            anyhow::bail!("managed top-level navigation was denied")
        }
    }
    Ok(())
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

fn invalid_interception_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<chromiumoxide::error::CdpError>()
        .is_some_and(|error| {
            matches!(
                error,
                chromiumoxide::error::CdpError::Chrome(response)
                    if response.code == -32602 && response.message == "Invalid InterceptionId."
            )
        })
}

fn discardable_obsolete_interception(
    error: &anyhow::Error,
    request_loader: Option<&str>,
    current_loader: &str,
) -> bool {
    invalid_interception_error(error)
        && request_loader.is_some_and(|loader| !loader.is_empty())
        && request_loader != Some(current_loader)
}

const MEDIA_GUARD_BOOTSTRAP: &str = r#"(() => {
    if (globalThis.__omarchyKidsBlockMediaForImage) return true;
    const blockedUrls = new Set();
    const imageIsBlocked = image =>
        image.hasAttribute("data-omarchy-kids-blocked-image") ||
        blockedUrls.has(image.currentSrc) ||
        blockedUrls.has(image.src);
    const apply = () => {
        const root = document.documentElement;
        if (!root || blockedUrls.size === 0) return;
        if (!root.hasAttribute("data-omarchy-kids-media-blocked")) {
            root.setAttribute("data-omarchy-kids-media-blocked", "");
        }
        for (const image of document.images) {
            if (imageIsBlocked(image)) {
                image.setAttribute("data-omarchy-kids-blocked-image", "");
                const action = image.closest("a,button,[role='button']");
                if (action) action.setAttribute("data-omarchy-kids-blocked-media-trigger", "");
            }
        }
        for (const video of document.querySelectorAll("video")) {
            video.removeAttribute("autoplay");
            video.pause();
        }
    };
    const connectedToBlockedImage = target => {
        if (!(target instanceof Element)) return false;
        const action = target.closest("a,button,[role='button']");
        if (!action) return target.matches("img") && imageIsBlocked(target);
        return action.hasAttribute("data-omarchy-kids-blocked-media-trigger") ||
            Array.from(action.querySelectorAll("img")).some(imageIsBlocked);
    };
    const blockConnectedAction = event => {
        if (event.type === "keydown" && !["Enter", " "].includes(event.key)) return;
        if (document.documentElement?.hasAttribute("data-omarchy-kids-media-blocked") &&
            connectedToBlockedImage(event.target)) {
            event.preventDefault();
            event.stopImmediatePropagation();
        }
    };
    const stopVideo = event => {
        if (document.documentElement?.hasAttribute("data-omarchy-kids-media-blocked") &&
            event.target instanceof HTMLVideoElement) {
            event.target.pause();
        }
    };
    window.addEventListener("click", blockConnectedAction, true);
    window.addEventListener("auxclick", blockConnectedAction, true);
    window.addEventListener("keydown", blockConnectedAction, true);
    window.addEventListener("play", stopVideo, true);
    new MutationObserver(apply).observe(document, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ["data-omarchy-kids-media-blocked"]
    });
    const blockMediaForImage = blockedUrl => {
        blockedUrls.add(blockedUrl);
        apply();
        return document.documentElement?.hasAttribute("data-omarchy-kids-media-blocked") &&
            Array.from(document.querySelectorAll("video")).every(video => video.paused);
    };
    Object.defineProperty(globalThis, "__omarchyKidsBlockMediaForImage", {
        value: blockMediaForImage,
        configurable: false,
        writable: false
    });
    return true;
})()"#;

fn media_block_expression(blocked_url: &str) -> Result<String> {
    let encoded_url = serde_json::to_string(blocked_url)
        .context("failed to encode a blocked image URL for the media guard")?;
    Ok(format!(
        "globalThis.__omarchyKidsBlockMediaForImage({encoded_url})"
    ))
}

const MAX_REQUEST_LOADER_RECORDS: usize = 1024;

#[derive(Default)]
struct RequestLoaderLedger {
    order: VecDeque<NetworkRequestId>,
    loaders: HashMap<NetworkRequestId, LoaderId>,
}

impl RequestLoaderLedger {
    fn record(&mut self, request_id: NetworkRequestId, loader_id: LoaderId) {
        if self.loaders.insert(request_id.clone(), loader_id).is_some() {
            return;
        }
        self.order.push_back(request_id);
        if self.order.len() > MAX_REQUEST_LOADER_RECORDS
            && let Some(oldest) = self.order.pop_front()
        {
            self.loaders.remove(&oldest);
        }
    }

    fn take(&mut self, request_id: &NetworkRequestId) -> Option<LoaderId> {
        let loader = self.loaders.remove(request_id)?;
        if let Some(index) = self
            .order
            .iter()
            .position(|candidate| candidate == request_id)
        {
            self.order.remove(index);
        }
        Some(loader)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ManualCounts {
    intercepted: usize,
    continued: usize,
    replaced: usize,
    failed_closed: usize,
    canceled: usize,
    media_blocked_documents: usize,
    domain_blocked_requests: usize,
    safe_search_rewrites: usize,
    youtube_restricted_requests: usize,
}

fn record_request_outcome(outcome: RequestLayerOutcome, counts: &mut ManualCounts) {
    match outcome {
        RequestLayerOutcome::Continued => {}
        RequestLayerOutcome::DomainBlocked => counts.domain_blocked_requests += 1,
        RequestLayerOutcome::SafeSearchRewritten => counts.safe_search_rewrites += 1,
        RequestLayerOutcome::YouTubeRestricted => counts.youtube_restricted_requests += 1,
    }
}

#[cfg(test)]
fn settle_request_decision(
    outcome: RequestLayerOutcome,
    counts: &mut ManualCounts,
    resolve: impl FnOnce() -> Result<()>,
) -> Result<()> {
    resolve()?;
    record_request_outcome(outcome, counts);
    Ok(())
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
    request_loaders: RequestLoaderLedger,
    media_blocked_loader: Option<String>,
    counts: ManualCounts,
}

impl ManualPageState {
    fn navigation_started(&mut self, loader_id: &str) {
        if self.active_loader.as_deref() == Some(loader_id) {
            return;
        }
        self.active_loader = Some(loader_id.to_owned());
        self.loaded_loader = None;
        self.media_blocked_loader = None;
    }

    fn begin_response(&mut self, request_id: &RequestId) -> Result<()> {
        self.ledger.begin(request_id)
    }

    fn cancel_obsolete(&mut self, request_id: &RequestId) -> Result<()> {
        self.ledger.resolved(request_id)?;
        self.counts.intercepted += 1;
        self.counts.canceled += 1;
        Ok(())
    }

    fn mark_media_blocked(&mut self, loader_id: &str) -> bool {
        if self.active_loader.as_deref() != Some(loader_id)
            || self.media_blocked_loader.as_deref() == Some(loader_id)
        {
            return false;
        }
        self.media_blocked_loader = Some(loader_id.to_owned());
        self.counts.media_blocked_documents += 1;
        true
    }

    #[cfg(test)]
    fn media_blocked(&self) -> bool {
        self.active_loader.is_some() && self.active_loader == self.media_blocked_loader
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
    pub canceled: usize,
    pub media_blocked_documents: usize,
    pub domain_blocked_requests: usize,
    pub safe_search_rewrites: usize,
    pub youtube_restricted_requests: usize,
    pub blocklist_entries: usize,
    pub blocked_extra_pages: usize,
    pub unresolved: usize,
    pub clean_shutdown: bool,
}

pub struct ManagedBrowser {
    detector: Detector,
    policy: Policy,
    request_policy: RequestPolicy,
}

impl ManagedBrowser {
    pub fn new(detector: Detector, policy: Policy, request_policy: RequestPolicy) -> Self {
        Self {
            detector,
            policy,
            request_policy,
        }
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

fn build_managed_browser_config(config: &ManagedBrowserConfig) -> Result<BrowserConfig> {
    BrowserConfig::builder()
        .chrome_executable(&config.chromium_bin)
        .with_head()
        .arg("disable-dev-tools")
        .arg(managed_chromium_argument())
        .user_data_dir(&config.profile_dir)
        .extension(config.extension_dir.display().to_string())
        .window_size(1280, 800)
        .launch_timeout(config.lifecycle_timeout)
        .request_timeout(config.acquisition_timeout)
        .disable_cache()
        .build()
        .map_err(anyhow::Error::msg)
}

async fn run_managed_browser(
    managed: ManagedBrowser,
    config: ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    let ManagedBrowser {
        detector,
        policy,
        request_policy,
    } = managed;
    let browser_config = build_managed_browser_config(&config)?;
    let detector_session = HeadedDetectorSession::start(detector);

    let browser_result = match Browser::launch(browser_config).await {
        Ok((browser, handler)) => {
            run_managed_with_browser(
                browser,
                handler,
                &detector_session,
                &policy,
                &request_policy,
                &config,
            )
            .await
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
    request_policy: &RequestPolicy,
    config: &ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            event.context("Chromiumoxide handler failed")?;
        }
        Ok::<_, anyhow::Error>(())
    });

    let run_result = managed_event_loop(
        &mut browser,
        detector_session,
        policy,
        request_policy,
        config,
    )
    .await;
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
    request_policy: &RequestPolicy,
    config: &ManagedBrowserConfig,
) -> Result<ManagedBrowserSummary> {
    within(
        config.acquisition_timeout,
        browser.execute(managed_download_behavior()),
        "download policy deadline elapsed",
    )
    .await?
    .context("failed to deny managed-browser downloads")?;

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
    let mut requests = within(
        config.acquisition_timeout,
        page.event_listener::<EventRequestWillBeSent>(),
        "Network.requestWillBeSent listener registration deadline elapsed",
    )
    .await?
    .context("failed to register Network.requestWillBeSent listener")?;
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

    install_media_guard(&page, config.acquisition_timeout).await?;
    within(
        config.acquisition_timeout,
        page.execute(SetLifecycleEventsEnabledParams::new(true)),
        "Page.setLifecycleEventsEnabled deadline elapsed",
    )
    .await?
    .context("failed to enable page lifecycle events")?;

    let mut state = ManualPageState::default();
    let initial = current_main_frame(&page, config.acquisition_timeout).await?;
    state.navigation_started(initial.1.as_ref());
    state.load_completed(initial.1.as_ref());
    reveal_current_document(&page, config.acquisition_timeout).await?;

    // The page is still the local about:blank target here. Establish its frame
    // state before enabling the catch-all request-stage interceptor; every
    // externally navigated request is still covered because navigation begins
    // only after Fetch.enable completes below.
    within(
        config.acquisition_timeout,
        page.execute(managed_fetch_enable_params()),
        "Fetch.enable deadline elapsed",
    )
    .await?
    .context("failed to enable image response interception")?;
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
                match pause_stage(
                    event.response_status_code,
                    event.response_error_reason.is_some(),
                ) {
                    PauseStage::Request => {
                        process_managed_request_pause(
                            &page,
                            event.as_ref(),
                            request_policy,
                            config,
                            &mut state,
                            &mut requests,
                        ).await?;
                    }
                    PauseStage::Response => {
                        process_managed_response_pause(
                            &page,
                            event.as_ref(),
                            detector_session,
                            policy,
                            config,
                            &mut state,
                            &mut requests,
                        ).await?;
                    }
                }
                reveal_if_current_and_ready(&page, config, &state).await?;
            }
            event = requests.next() => {
                let Some(event) = event else { break; };
                state.request_loaders.record(event.request_id.clone(), event.loader_id.clone());
            }
            event = frames.next() => {
                let Some(event) = event else { break; };
                apply_navigation_decision(
                    navigation_decision(event.frame.parent_id.is_some(), &event.frame.url),
                    event.frame.loader_id.as_ref(),
                    &mut state,
                )?;
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
        canceled: state.counts.canceled,
        media_blocked_documents: state.counts.media_blocked_documents,
        domain_blocked_requests: state.counts.domain_blocked_requests,
        safe_search_rewrites: state.counts.safe_search_rewrites,
        youtube_restricted_requests: state.counts.youtube_restricted_requests,
        blocklist_entries: request_policy.blocklist_entries(),
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

async fn install_media_guard(page: &Page, deadline: Duration) -> Result<()> {
    let params = AddScriptToEvaluateOnNewDocumentParams {
        source: MEDIA_GUARD_BOOTSTRAP.to_owned(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: Some(true),
    };
    within(
        deadline,
        page.execute(params),
        "managed media guard installation deadline elapsed",
    )
    .await?
    .context("failed to install the managed media guard")?;
    let result = within(
        deadline,
        page.evaluate("typeof globalThis.__omarchyKidsBlockMediaForImage === 'function'"),
        "managed media guard verification deadline elapsed",
    )
    .await?
    .context("failed to verify the managed media guard")?;
    let installed = result
        .into_value::<bool>()
        .context("managed media guard verification had an unexpected shape")?;
    anyhow::ensure!(installed, "managed media guard was not installed");
    Ok(())
}

async fn process_managed_response_pause(
    page: &Page,
    event: &EventRequestPaused,
    detector_session: &HeadedDetectorSession,
    policy: &Policy,
    config: &ManagedBrowserConfig,
    state: &mut ManualPageState,
    requests: &mut EventStream<EventRequestWillBeSent>,
) -> Result<()> {
    state.begin_response(&event.request_id)?;
    let classification =
        prepare_managed_classification(page, event, detector_session, policy, config).await;
    let request_loader = request_loader_for(
        event.network_id.as_ref(),
        requests,
        &mut state.request_loaders,
        config.acquisition_timeout,
    )
    .await;
    let replace = classification != Classification::Allow;
    let resolution = if replace {
        match within(
            config.acquisition_timeout,
            page.execute(replacement_response(event.request_id.clone())),
            "Fetch.fulfillRequest deadline elapsed",
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(anyhow::Error::from(error)),
            Err(error) => Err(error),
        }
    } else {
        match within(
            config.acquisition_timeout,
            page.execute(continue_response(event.request_id.clone())),
            "Fetch.continueResponse deadline elapsed",
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(anyhow::Error::from(error)),
            Err(error) => Err(error),
        }
    };
    if let Err(error) = resolution {
        if invalid_interception_error(&error)
            && let Ok((_, current_loader)) =
                current_main_frame(page, config.acquisition_timeout).await
            && discardable_obsolete_interception(
                &error,
                request_loader.as_ref().map(LoaderId::as_ref),
                current_loader.as_ref(),
            )
        {
            state.cancel_obsolete(&event.request_id)?;
            return Ok(());
        }
        return Err(error).context(if replace {
            "failed to fulfill a managed image response"
        } else {
            "failed to continue a managed image response"
        });
    }
    if replace {
        let request_loader = request_loader
            .as_ref()
            .filter(|loader| !loader.as_ref().is_empty())
            .context("replaced image response had no document loader identity")?;
        let (_, current_loader) = current_main_frame(page, config.acquisition_timeout).await?;
        if request_loader == &current_loader {
            block_media_for_current_document(page, &event.request.url, config.acquisition_timeout)
                .await?;
            state.navigation_started(current_loader.as_ref());
            state.mark_media_blocked(current_loader.as_ref());
        }
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

async fn process_managed_request_pause(
    page: &Page,
    event: &EventRequestPaused,
    request_policy: &RequestPolicy,
    config: &ManagedBrowserConfig,
    state: &mut ManualPageState,
    requests: &mut EventStream<EventRequestWillBeSent>,
) -> Result<()> {
    let request_url = Url::parse(&event.request.url).context("managed request URL was invalid")?;
    let headers = network_headers(&event.request.headers)?;
    let decision = request_policy.evaluate(&request_url, &headers)?;
    let outcome = request_outcome(&decision);
    let command = request_command(event.request_id.clone(), decision);
    let resolution = match command {
        RequestCommand::Fail(params) => match within(
            config.acquisition_timeout,
            page.execute(params),
            "Fetch.failRequest deadline elapsed",
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(anyhow::Error::from(error)),
            Err(error) => Err(error),
        },
        RequestCommand::Continue(params) => match within(
            config.acquisition_timeout,
            page.execute(params),
            "Fetch.continueRequest deadline elapsed",
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(anyhow::Error::from(error)),
            Err(error) => Err(error),
        },
    };
    if let Err(error) = resolution {
        let request_loader = request_loader_for(
            event.network_id.as_ref(),
            requests,
            &mut state.request_loaders,
            config.acquisition_timeout,
        )
        .await;
        if invalid_interception_error(&error)
            && let Ok((_, current_loader)) =
                current_main_frame(page, config.acquisition_timeout).await
            && discardable_obsolete_interception(
                &error,
                request_loader.as_ref().map(LoaderId::as_ref),
                current_loader.as_ref(),
            )
        {
            return Ok(());
        }
        return Err(error).context("failed to settle a managed request before network transfer");
    }

    record_request_outcome(outcome, &mut state.counts);
    if outcome == RequestLayerOutcome::DomainBlocked
        && event.resource_type == chromiumoxide::cdp::browser_protocol::network::ResourceType::Image
    {
        let (current_frame, current_loader) =
            current_main_frame(page, config.acquisition_timeout).await?;
        if event.frame_id == current_frame {
            block_media_for_current_document(page, &event.request.url, config.acquisition_timeout)
                .await?;
            state.navigation_started(current_loader.as_ref());
            state.mark_media_blocked(current_loader.as_ref());
        }
    }
    Ok(())
}

async fn block_media_for_current_document(
    page: &Page,
    blocked_url: &str,
    deadline: Duration,
) -> Result<()> {
    let expression = media_block_expression(blocked_url)?;
    let result = within(
        deadline,
        page.evaluate(expression),
        "managed media blocking deadline elapsed",
    )
    .await?
    .context("failed to install managed media blocking")?;
    let blocked = result
        .into_value::<bool>()
        .context("managed media blocking result had an unexpected shape")?;
    anyhow::ensure!(blocked, "managed media blocking was not applied");
    Ok(())
}

async fn request_loader_for(
    request_id: Option<&NetworkRequestId>,
    requests: &mut EventStream<EventRequestWillBeSent>,
    request_loaders: &mut RequestLoaderLedger,
    deadline: Duration,
) -> Option<LoaderId> {
    let request_id = request_id?;
    if let Some(loader) = request_loaders.take(request_id) {
        return Some(loader);
    }
    tokio::time::timeout(deadline, async {
        loop {
            let event = requests.next().await?;
            request_loaders.record(event.request_id.clone(), event.loader_id.clone());
            if let Some(loader) = request_loaders.take(request_id) {
                return Some(loader);
            }
        }
    })
    .await
    .ok()
    .flatten()
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
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

    use chromiumoxide::cdp::browser_protocol::{
        fetch::RequestId,
        network::{Headers, LoaderId, RequestId as NetworkRequestId},
    };
    use tempfile::TempDir;
    use url::Url;

    use crate::{
        inference::{Detection, InferenceReport},
        policy::Policy,
    };

    use super::{
        Classification, MEDIA_GUARD_BOOTSTRAP, ManagedBrowserConfig, ManagedBrowserSummary,
        ManualCounts, ManualPageState, ManualPauseLedger, NavigationDecision, PauseStage,
        RequestLayerOutcome, RequestLoaderLedger, ResponsePlan, TargetAction,
        apply_navigation_decision, build_managed_browser_config, classification_for,
        discardable_obsolete_interception, managed_download_behavior, media_block_expression,
        navigation_decision, network_headers, pause_stage, request_command, response_plan,
        reveal_failure_is_stale, settle_decision, settle_request_decision, should_reveal,
        supports_complete_frame_analysis, target_action,
    };
    use crate::request_policy::RequestDecision;

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
    fn top_level_navigation_allows_only_complete_http_and_https_urls() {
        for url in [
            "http://example.test/",
            "https://example.test/path?private=query#fragment",
        ] {
            assert_eq!(
                navigation_decision(false, url),
                NavigationDecision::ContinueMainFrame,
                "{url}"
            );
        }

        for url in [
            "",
            "not a url",
            "about:blank",
            "file:///etc/passwd",
            "ftp://example.test/file",
            "javascript:alert(1)",
            "data:text/html,unsafe",
            "blob:https://example.test/id",
            "view-source:https://example.test/",
            "devtools://devtools/bundled/inspector.html",
            "chrome://settings/",
            "chrome-untrusted://new-tab-page/",
        ] {
            assert_eq!(
                navigation_decision(false, url),
                NavigationDecision::DenyMainFrame,
                "{url}"
            );
        }
    }

    #[test]
    fn subframe_navigation_never_changes_main_frame_state() {
        for url in [
            "https://frame.example.test/",
            "data:text/html,frame",
            "malformed frame url",
        ] {
            assert_eq!(
                navigation_decision(true, url),
                NavigationDecision::IgnoreSubframe,
                "{url}"
            );
        }
    }

    #[test]
    fn navigation_transition_updates_only_an_allowed_main_frame() {
        let mut state = ManualPageState::default();
        state.navigation_started("original-loader");
        state.load_completed("original-loader");
        assert!(state.ready_to_reveal());

        apply_navigation_decision(
            NavigationDecision::IgnoreSubframe,
            "subframe-loader",
            &mut state,
        )
        .unwrap();
        assert!(state.ready_to_reveal());

        apply_navigation_decision(
            NavigationDecision::ContinueMainFrame,
            "next-loader",
            &mut state,
        )
        .unwrap();
        assert!(!state.ready_to_reveal());

        let error = apply_navigation_decision(
            NavigationDecision::DenyMainFrame,
            "denied-loader",
            &mut state,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "managed top-level navigation was denied");

        let source = include_str!("managed.rs");
        assert!(source.contains(
            "apply_navigation_decision(\n                    navigation_decision(event.frame.parent_id.is_some(), &event.frame.url),"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_chromium_is_forced_onto_native_wayland() {
        let fixture = ConfigFixture::new();
        let argument_log = fixture.root.path().join("arguments");
        fs::write(
            &fixture.chromium,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                argument_log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fixture.chromium, fs::Permissions::from_mode(0o755)).unwrap();

        let config = fixture.build(None).unwrap();
        let mut child = build_managed_browser_config(&config)
            .unwrap()
            .launch()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("fake Chromium did not exit")
            .unwrap();

        let arguments = fs::read_to_string(argument_log).unwrap();
        let mut safety_arguments = arguments
            .lines()
            .filter(|argument| {
                *argument == "--disable-dev-tools" || argument.starts_with("--ozone-platform")
            })
            .collect::<Vec<_>>();
        safety_arguments.sort_unstable();
        assert_eq!(
            safety_arguments,
            ["--disable-dev-tools", "--ozone-platform=wayland"]
        );
    }

    #[test]
    fn managed_browser_denies_downloads_before_navigation() {
        assert_eq!(
            serde_json::to_value(managed_download_behavior()).unwrap(),
            serde_json::json!({"behavior": "deny"})
        );
        let source = include_str!("managed.rs");
        let production_call = ["browser.execute(", "managed_download_behavior()", ")"].concat();
        assert_eq!(source.matches(&production_call).count(), 1);
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
    fn fetch_pause_stage_follows_cdp_presence_semantics() {
        assert_eq!(pause_stage(None, false), PauseStage::Request);
        assert_eq!(pause_stage(Some(200), false), PauseStage::Response);
        assert_eq!(pause_stage(None, true), PauseStage::Response);
        assert_eq!(pause_stage(Some(500), true), PauseStage::Response);
    }

    #[test]
    fn request_commands_fail_domains_and_minimally_override_hardened_requests() {
        let request = request_id("request-stage");
        let cases = [
            (
                RequestDecision::BlockDomain,
                serde_json::json!({
                    "requestId": "request-stage",
                    "errorReason": "BlockedByClient"
                }),
                true,
            ),
            (
                RequestDecision::Continue,
                serde_json::json!({ "requestId": "request-stage" }),
                false,
            ),
            (
                RequestDecision::RewriteUrl(
                    "https://www.google.com/search?q=x&safe=active&ssui=on".to_owned(),
                ),
                serde_json::json!({
                    "requestId": "request-stage",
                    "url": "https://www.google.com/search?q=x&safe=active&ssui=on"
                }),
                false,
            ),
            (
                RequestDecision::ReplaceHeaders(vec![
                    ("Accept".to_owned(), "text/html".to_owned()),
                    ("YouTube-Restrict".to_owned(), "Strict".to_owned()),
                ]),
                serde_json::json!({
                    "requestId": "request-stage",
                    "headers": [
                        { "name": "Accept", "value": "text/html" },
                        { "name": "YouTube-Restrict", "value": "Strict" }
                    ]
                }),
                false,
            ),
        ];

        for (decision, expected, should_fail) in cases {
            let command = request_command(request.clone(), decision);
            assert_eq!(command.is_fail(), should_fail);
            assert_eq!(command.json(), expected);
        }
    }

    #[test]
    fn request_counts_change_only_after_cdp_settlement_succeeds() {
        let mut counts = ManualCounts::default();
        let error =
            settle_request_decision(RequestLayerOutcome::DomainBlocked, &mut counts, || {
                anyhow::bail!("CDP broke")
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "CDP broke");
        assert_eq!(counts, ManualCounts::default());

        for outcome in [
            RequestLayerOutcome::Continued,
            RequestLayerOutcome::DomainBlocked,
            RequestLayerOutcome::SafeSearchRewritten,
            RequestLayerOutcome::YouTubeRestricted,
        ] {
            settle_request_decision(outcome, &mut counts, || Ok(())).unwrap();
        }
        assert_eq!(counts.domain_blocked_requests, 1);
        assert_eq!(counts.safe_search_rewrites, 1);
        assert_eq!(counts.youtube_restricted_requests, 1);
    }

    #[test]
    fn network_headers_are_preserved_only_when_their_shape_is_textual() {
        let headers = Headers::new(serde_json::json!({
            "Accept": "text/html",
            "Cookie": "private=value"
        }));
        let mut actual = network_headers(&headers).unwrap();
        actual.sort();
        assert_eq!(
            actual,
            [
                ("Accept".to_owned(), "text/html".to_owned()),
                ("Cookie".to_owned(), "private=value".to_owned()),
            ]
        );

        for malformed in [
            Headers::new(serde_json::json!([])),
            Headers::new(serde_json::json!({ "Accept": 7 })),
        ] {
            assert!(network_headers(&malformed).is_err());
        }
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
    fn invalid_interception_is_discarded_only_for_an_obsolete_request_loader() {
        let invalid = anyhow::Error::new(chromiumoxide::error::CdpError::Chrome(
            chromiumoxide::types::Error {
                code: -32602,
                message: "Invalid InterceptionId.".to_owned(),
            },
        ));
        assert!(discardable_obsolete_interception(
            &invalid,
            Some("loader-old"),
            "loader-current"
        ));
        assert!(!discardable_obsolete_interception(
            &invalid,
            Some("loader-current"),
            "loader-current"
        ));
        assert!(!discardable_obsolete_interception(
            &invalid,
            None,
            "loader-current"
        ));
        assert!(!discardable_obsolete_interception(
            &invalid,
            Some(""),
            "loader-current"
        ));
        assert!(!discardable_obsolete_interception(
            &anyhow::anyhow!("transport failed"),
            Some("loader-old"),
            "loader-current"
        ));
    }

    #[test]
    fn request_loader_evidence_is_exact_and_consumed_once() {
        let requested = NetworkRequestId::from("requested".to_owned());
        let other = NetworkRequestId::from("other".to_owned());
        let old_loader = LoaderId::from("loader-old".to_owned());
        let other_loader = LoaderId::from("loader-other".to_owned());
        let mut ledger = RequestLoaderLedger::default();

        ledger.record(other.clone(), other_loader.clone());
        ledger.record(requested.clone(), old_loader.clone());
        assert_eq!(ledger.take(&requested), Some(old_loader));
        assert_eq!(ledger.take(&requested), None);
        assert_eq!(ledger.take(&other), Some(other_loader));
    }

    #[test]
    fn request_loader_history_is_bounded() {
        let mut ledger = RequestLoaderLedger::default();
        for index in 0..=1024 {
            ledger.record(
                NetworkRequestId::from(format!("request-{index}")),
                LoaderId::from(format!("loader-{index}")),
            );
        }

        assert_eq!(
            ledger.take(&NetworkRequestId::from("request-0".to_owned())),
            None
        );
        assert_eq!(
            ledger.take(&NetworkRequestId::from("request-1024".to_owned())),
            Some(LoaderId::from("loader-1024".to_owned()))
        );
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
            canceled: 1,
            media_blocked_documents: 1,
            domain_blocked_requests: 2,
            safe_search_rewrites: 3,
            youtube_restricted_requests: 4,
            blocklist_entries: 76_767,
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
                "blocklist_entries",
                "canceled",
                "chromium_version",
                "clean_shutdown",
                "continued",
                "domain_blocked_requests",
                "failed_closed",
                "intercepted",
                "media_blocked_documents",
                "model_sha256",
                "onnx_runtime_version",
                "replaced",
                "safe_search_rewrites",
                "unresolved",
                "youtube_restricted_requests",
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
    fn canceled_obsolete_pause_is_accounted_without_allowing_or_replacing() {
        let request = request_id("obsolete");
        let mut state = ManualPageState::default();
        state.begin_response(&request).unwrap();
        state.cancel_obsolete(&request).unwrap();

        assert_eq!(state.ledger.unresolved_count(), 0);
        assert_eq!(state.counts.intercepted, 1);
        assert_eq!(state.counts.canceled, 1);
        assert_eq!(state.counts.continued, 0);
        assert_eq!(state.counts.replaced, 0);
        assert_eq!(state.counts.failed_closed, 0);
    }

    #[test]
    fn media_blocking_is_counted_once_per_document_and_resets_on_navigation() {
        let mut state = ManualPageState::default();
        state.navigation_started("loader-a");
        assert!(state.mark_media_blocked("loader-a"));
        assert!(!state.mark_media_blocked("loader-a"));
        assert_eq!(state.counts.media_blocked_documents, 1);

        state.navigation_started("loader-b");
        assert!(!state.media_blocked());
        assert!(!state.mark_media_blocked("loader-a"));
        assert!(state.mark_media_blocked("loader-b"));
        assert_eq!(state.counts.media_blocked_documents, 2);
    }

    #[test]
    fn media_block_expression_binds_the_url_and_guards_video_and_connected_actions() {
        let url = "https://example.test/a'\"\\image.png";
        let expression = media_block_expression(url).unwrap();
        let encoded_url = serde_json::to_string(url).unwrap();

        assert_eq!(
            expression,
            format!("globalThis.__omarchyKidsBlockMediaForImage({encoded_url})")
        );
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("document.querySelectorAll(\"video\")"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("video.pause()"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("window.addEventListener(\"click\""));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("window.addEventListener(\"auxclick\""));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("window.addEventListener(\"keydown\""));
        assert!(
            MEDIA_GUARD_BOOTSTRAP
                .contains("event.type === \"keydown\" && ![\"Enter\", \" \"].includes(event.key)")
        );
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("event.preventDefault()"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("event.stopImmediatePropagation()"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("data-omarchy-kids-media-blocked"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("data-omarchy-kids-blocked-media-trigger"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("new MutationObserver(apply)"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("attributes: true"));
        assert!(MEDIA_GUARD_BOOTSTRAP.contains("Object.defineProperty(globalThis"));
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
