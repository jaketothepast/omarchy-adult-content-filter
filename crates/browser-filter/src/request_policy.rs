use anyhow::{Context, Result};
use url::{Host, Url};

use crate::domain_policy::DomainPolicy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestDecision {
    BlockDomain,
    Continue,
    RewriteUrl(String),
    ReplaceHeaders(Vec<(String, String)>),
}

pub struct RequestPolicy {
    domains: DomainPolicy,
}

impl RequestPolicy {
    pub fn new(domains: DomainPolicy) -> Self {
        Self { domains }
    }

    pub fn evaluate(&self, url: &Url, headers: &[(String, String)]) -> Result<RequestDecision> {
        if !matches!(url.scheme(), "http" | "https") {
            return Ok(RequestDecision::Continue);
        }
        let host = url.host().context("HTTP(S) request URL has no host")?;
        if let Host::Domain(host) = host
            && self.domains.contains_host(host)
        {
            return Ok(RequestDecision::BlockDomain);
        }
        if should_force_google_safe_search(url) {
            let rewritten = force_google_safe_search(url);
            if rewritten != *url {
                return Ok(RequestDecision::RewriteUrl(rewritten.into()));
            }
        }
        if should_force_youtube_restricted_mode(url) {
            let mut restricted = headers
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("YouTube-Restrict"))
                .cloned()
                .collect::<Vec<_>>();
            restricted.push(("YouTube-Restrict".to_owned(), "Strict".to_owned()));
            return Ok(RequestDecision::ReplaceHeaders(restricted));
        }
        Ok(RequestDecision::Continue)
    }

    pub fn blocklist_entries(&self) -> usize {
        self.domains.entry_count()
    }
}

fn should_force_youtube_restricted_mode(url: &Url) -> bool {
    url.port().is_none()
        && url.host_str().is_some_and(|host| {
            host == "youtube.com"
                || host.ends_with(".youtube.com")
                || host == "youtube-nocookie.com"
                || host.ends_with(".youtube-nocookie.com")
        })
}

fn should_force_google_safe_search(url: &Url) -> bool {
    url.port().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "google.com" || host.ends_with(".google.com"))
        && matches!(url.path(), "/" | "/search" | "/webhp" | "/safesearch")
}

fn force_google_safe_search(url: &Url) -> Url {
    let retained = url
        .query_pairs()
        .filter(|(name, _)| {
            !name.eq_ignore_ascii_case("safe") && !name.eq_ignore_ascii_case("ssui")
        })
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut rewritten = url.clone();
    rewritten.set_query(None);
    rewritten
        .query_pairs_mut()
        .extend_pairs(retained)
        .append_pair("safe", "active")
        .append_pair("ssui", "on");
    rewritten
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use url::Url;

    use crate::domain_policy::DomainPolicy;

    use super::{RequestDecision, RequestPolicy};

    struct PolicyFixture {
        _root: TempDir,
        policy: RequestPolicy,
    }

    impl PolicyFixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("adult-domains.hosts");
            fs::write(
                &path,
                b"0.0.0.0 blocked.example.test\n0.0.0.0 media.blocked.test\n",
            )
            .unwrap();
            let policy = RequestPolicy::new(DomainPolicy::load(&path).unwrap());
            Self {
                _root: root,
                policy,
            }
        }

        fn evaluate(&self, url: &str) -> RequestDecision {
            self.policy
                .evaluate(&Url::parse(url).unwrap(), &[])
                .unwrap()
        }
    }

    #[test]
    fn blocks_listed_domains_and_subdomains_independent_of_resource_path() {
        let fixture = PolicyFixture::new();
        for url in [
            "http://blocked.example.test/",
            "https://blocked.example.test/image.png",
            "https://cdn.blocked.example.test/script.js",
            "https://deep.media.blocked.test/video.mp4?autoplay=1",
        ] {
            assert_eq!(fixture.evaluate(url), RequestDecision::BlockDomain, "{url}");
        }
    }

    #[test]
    fn safe_and_lookalike_hosts_continue_unchanged() {
        let fixture = PolicyFixture::new();
        for url in [
            "https://safe.example.test/",
            "https://notblocked.example.test/",
            "https://blocked.example.test.example/",
            "http://127.0.0.1:8080/fixture",
            "http://[::1]:8080/fixture",
        ] {
            assert_eq!(fixture.evaluate(url), RequestDecision::Continue, "{url}");
        }
    }

    #[test]
    fn non_web_schemes_are_outside_the_request_policy() {
        let fixture = PolicyFixture::new();
        for url in ["data:text/plain,safe", "file:///tmp/example", "about:blank"] {
            assert_eq!(fixture.evaluate(url), RequestDecision::Continue, "{url}");
        }
    }

    #[test]
    fn google_search_urls_force_one_active_safe_search_pair() {
        let fixture = PolicyFixture::new();
        for (input, expected) in [
            (
                "https://www.google.com/search?q=space+cats&safe=off&SAFE=strict&ssui=off#results",
                "https://www.google.com/search?q=space+cats&safe=active&ssui=on#results",
            ),
            (
                "https://google.com/",
                "https://google.com/?safe=active&ssui=on",
            ),
            (
                "https://images.google.com/webhp?hl=en",
                "https://images.google.com/webhp?hl=en&safe=active&ssui=on",
            ),
            (
                "https://www.google.com/safesearch?safe=off&ssui=off",
                "https://www.google.com/safesearch?safe=active&ssui=on",
            ),
        ] {
            assert_eq!(
                fixture.evaluate(input),
                RequestDecision::RewriteUrl(expected.to_owned()),
                "{input}"
            );
        }
        assert_eq!(
            fixture.evaluate("https://www.google.com/search?q=x&safe=active&ssui=on"),
            RequestDecision::Continue
        );
    }

    #[test]
    fn google_rewrite_rejects_lookalikes_nonstandard_ports_and_other_services() {
        let fixture = PolicyFixture::new();
        for url in [
            "https://google.com.example/search?q=x",
            "https://notgoogle.com/search?q=x",
            "https://google.co.uk/search?q=x",
            "https://www.google.com:8443/search?q=x",
            "https://mail.google.com/mail/u/0/",
            "https://www.google.com/maps?q=x",
        ] {
            assert_eq!(fixture.evaluate(url), RequestDecision::Continue, "{url}");
        }
    }

    #[test]
    fn youtube_requests_receive_one_strict_header_without_losing_other_headers() {
        let fixture = PolicyFixture::new();
        let headers = vec![
            ("Accept".to_owned(), "text/html".to_owned()),
            ("youtube-restrict".to_owned(), "Off".to_owned()),
            ("Cookie".to_owned(), "session=private".to_owned()),
            ("YOUTUBE-RESTRICT".to_owned(), "Moderate".to_owned()),
        ];
        for url in [
            "https://youtube.com/",
            "https://www.youtube.com/watch?v=fixture",
            "https://music.youtube.com/",
            "https://www.youtube-nocookie.com/embed/fixture",
        ] {
            assert_eq!(
                fixture
                    .policy
                    .evaluate(&Url::parse(url).unwrap(), &headers)
                    .unwrap(),
                RequestDecision::ReplaceHeaders(vec![
                    ("Accept".to_owned(), "text/html".to_owned()),
                    ("Cookie".to_owned(), "session=private".to_owned()),
                    ("YouTube-Restrict".to_owned(), "Strict".to_owned()),
                ]),
                "{url}"
            );
        }
    }

    #[test]
    fn youtube_header_is_not_added_to_lookalikes_redirectors_or_nonstandard_ports() {
        let fixture = PolicyFixture::new();
        for url in [
            "https://youtube.com.example/watch",
            "https://notyoutube.com/watch",
            "https://youtu.be/fixture",
            "https://youtube.com:8443/watch",
        ] {
            assert_eq!(fixture.evaluate(url), RequestDecision::Continue, "{url}");
        }
    }

    #[test]
    fn domain_denial_precedes_service_hardening() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("adult-domains.hosts");
        fs::write(&path, b"0.0.0.0 google.com\n0.0.0.0 youtube.com\n").unwrap();
        let policy = RequestPolicy::new(DomainPolicy::load(&path).unwrap());

        for url in [
            "https://www.google.com/search?q=x",
            "https://www.youtube.com/watch?v=x",
        ] {
            assert_eq!(
                policy.evaluate(&Url::parse(url).unwrap(), &[]).unwrap(),
                RequestDecision::BlockDomain
            );
        }
    }
}
