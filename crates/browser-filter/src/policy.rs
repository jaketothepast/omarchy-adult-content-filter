use crate::inference::InferenceReport;
use url::Url;

const FIXTURE_MARKER: &str = "omarchy-kids-fixture";

#[derive(Default)]
pub struct Policy;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Replace { reason: &'static str },
}

impl Policy {
    pub fn decide(&self, request_url: &Url, _report: &InferenceReport) -> Verdict {
        if request_url.scheme() == "http"
            && request_url.host_str() == Some("127.0.0.1")
            && request_url
                .query_pairs()
                .any(|(name, value)| name == FIXTURE_MARKER && value == "flagged")
        {
            Verdict::Replace {
                reason: "deterministic-fixture",
            }
        } else {
            Verdict::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use crate::inference::InferenceReport;

    use super::{Policy, Verdict};

    fn completed_report() -> InferenceReport {
        InferenceReport {
            detections: Vec::new(),
            encoded_bytes: 70,
            width: 1,
            height: 1,
            decode_micros: 2,
            preprocess_micros: 3,
            inference_micros: 5,
            postprocess_micros: 7,
        }
    }

    // Production mutation caught: deciding before the completed inference result reaches policy,
    // or ignoring the fixture-only URL marker, would skip the required real inference layer.
    #[test]
    fn flagged_fixture_url_replaces_only_after_receiving_an_inference_report() {
        let request_url =
            Url::parse("http://127.0.0.1:40000/image/1.png?omarchy-kids-fixture=flagged").unwrap();

        assert_eq!(
            Policy.decide(&request_url, &completed_report()),
            Verdict::Replace {
                reason: "deterministic-fixture"
            }
        );
    }

    // Production mutation caught: treating any loopback image, instead of only the marked fixture,
    // as a replacement would make normal fixtures disappear from the controlled page.
    #[test]
    fn ordinary_urls_are_allowed_after_inference() {
        let request_url = Url::parse("http://127.0.0.1:40000/image/0.png").unwrap();

        assert_eq!(
            Policy.decide(&request_url, &completed_report()),
            Verdict::Allow
        );
    }

    // Production mutation caught: honoring the marker outside the controlled loopback fixture
    // origin would turn a deterministic experiment override into a policy for arbitrary URLs.
    #[test]
    fn marked_non_fixture_urls_are_allowed_after_inference() {
        let request_url =
            Url::parse("https://example.test/image.png?omarchy-kids-fixture=flagged").unwrap();

        assert_eq!(
            Policy.decide(&request_url, &completed_report()),
            Verdict::Allow
        );
    }
}
