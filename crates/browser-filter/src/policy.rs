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
    pub fn decide(&self, request_url: &Url, report: &InferenceReport) -> Verdict {
        if request_url.scheme() == "http"
            && request_url.host_str() == Some("127.0.0.1")
            && request_url
                .query_pairs()
                .any(|(name, value)| name == FIXTURE_MARKER && value == "flagged")
        {
            Verdict::Replace {
                reason: "deterministic-fixture",
            }
        } else if report.detections.iter().any(|detection| {
            matches!(
                detection.class_name,
                "BUTTOCKS_EXPOSED"
                    | "FEMALE_BREAST_EXPOSED"
                    | "FEMALE_GENITALIA_EXPOSED"
                    | "ANUS_EXPOSED"
                    | "MALE_GENITALIA_EXPOSED"
            )
        }) {
            Verdict::Replace {
                reason: "explicit-detection",
            }
        } else {
            Verdict::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use crate::inference::{Detection, InferenceReport};

    use super::{Policy, Verdict};

    fn completed_report() -> InferenceReport {
        report_with_detections(Vec::new())
    }

    fn report_with_detections(detections: Vec<Detection>) -> InferenceReport {
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

    fn detection(class_name: &'static str) -> Detection {
        Detection {
            class_name,
            score: 0.25,
            bounding_box: [0, 0, 1, 1],
        }
    }

    // Production mutation caught: deciding before the completed inference result reaches policy,
    // or ignoring the fixture-only URL marker, would skip the required real inference layer.
    #[test]
    fn flagged_fixture_url_replaces_only_after_receiving_an_inference_report() {
        let request_url =
            Url::parse("http://127.0.0.1:40000/image/1.png?omarchy-kids-fixture=flagged").unwrap();
        let report = report_with_detections(vec![detection("BUTTOCKS_EXPOSED")]);

        assert_eq!(
            Policy.decide(&request_url, &report),
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
        for request_url in [
            "https://127.0.0.1:40000/image.png?omarchy-kids-fixture=flagged",
            "http://localhost:40000/image.png?omarchy-kids-fixture=flagged",
            "http://127.0.0.1:40000/image.png?omarchy-kids-fixture=other",
        ] {
            assert_eq!(
                Policy.decide(&Url::parse(request_url).unwrap(), &completed_report()),
                Verdict::Allow,
                "fixture override must not apply to {request_url}"
            );
        }
    }

    // Production mutation caught: ignoring any already-thresholded exposed class would allow a
    // model detection that this narrow experiment policy is required to replace.
    #[test]
    fn exact_explicit_detection_classes_are_replaced() {
        let request_url = Url::parse("https://example.test/image.png").unwrap();

        for class_name in [
            "BUTTOCKS_EXPOSED",
            "FEMALE_BREAST_EXPOSED",
            "FEMALE_GENITALIA_EXPOSED",
            "ANUS_EXPOSED",
            "MALE_GENITALIA_EXPOSED",
        ] {
            let report = report_with_detections(vec![detection(class_name)]);
            assert_eq!(
                Policy.decide(&request_url, &report),
                Verdict::Replace {
                    reason: "explicit-detection"
                },
                "expected {class_name} to replace"
            );
        }
    }

    // Production mutation caught: matching all exposed or anatomically related model classes
    // would broaden this experiment policy beyond its five intentionally explicit classes.
    #[test]
    fn covered_ambiguous_and_near_match_classes_are_allowed() {
        let request_url = Url::parse("https://example.test/image.png").unwrap();

        for class_name in [
            "FEMALE_GENITALIA_COVERED",
            "FACE_FEMALE",
            "MALE_BREAST_EXPOSED",
            "FEET_EXPOSED",
            "BELLY_COVERED",
            "FEET_COVERED",
            "ARMPITS_COVERED",
            "ARMPITS_EXPOSED",
            "FACE_MALE",
            "BELLY_EXPOSED",
            "ANUS_COVERED",
            "FEMALE_BREAST_COVERED",
            "BUTTOCKS_COVERED",
            "BUTTOCKS_EXPOSED_EXTRA",
        ] {
            let report = report_with_detections(vec![detection(class_name)]);
            assert_eq!(
                Policy.decide(&request_url, &report),
                Verdict::Allow,
                "expected {class_name} to remain allowed"
            );
        }
    }

    // Production mutation caught: treating a completed empty report as suspicious would replace
    // ordinary images without any model evidence or fixture override.
    #[test]
    fn empty_detection_report_is_allowed() {
        let request_url = Url::parse("https://example.test/image.png").unwrap();

        assert_eq!(
            Policy.decide(&request_url, &completed_report()),
            Verdict::Allow
        );
    }
}
