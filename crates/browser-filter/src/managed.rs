use std::{fs, path::PathBuf, time::Duration};

use anyhow::Result;
use url::Url;

use crate::{
    inference::InferenceReport,
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

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use tempfile::TempDir;
    use url::Url;

    use crate::{
        inference::{Detection, InferenceReport},
        policy::Policy,
    };

    use super::{
        Classification, ManagedBrowserConfig, ResponsePlan, TargetAction, classification_for,
        response_plan, should_reveal, target_action,
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
            [Duration::ZERO, Duration::from_secs(5), Duration::from_secs(30)],
            [Duration::from_secs(5), Duration::ZERO, Duration::from_secs(30)],
            [Duration::from_secs(5), Duration::from_secs(5), Duration::ZERO],
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
            assert_eq!(error.to_string(), "Chromium profile must not be a default browser profile");
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
        for status in [None, Some(101), Some(204), Some(206), Some(304), Some(404), Some(500)] {
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
    fn reveal_requires_a_load_event_and_zero_unresolved_responses() {
        assert!(!should_reveal(false, 0));
        assert!(!should_reveal(true, 1));
        assert!(should_reveal(true, 0));
    }

    #[test]
    fn only_additional_page_targets_are_closed() {
        assert_eq!(target_action("primary", "primary", "page"), TargetAction::Keep);
        assert_eq!(target_action("primary", "popup", "page"), TargetAction::Close);
        assert_eq!(
            target_action("primary", "worker", "service_worker"),
            TargetAction::Ignore
        );
    }
}
