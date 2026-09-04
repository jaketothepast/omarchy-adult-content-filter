use std::{collections::HashSet, fs::File, io::Read, net::IpAddr, path::Path};

use anyhow::{Context, Result};

const MAX_POLICY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_POLICY_ENTRIES: usize = 250_000;
pub const PINNED_ADULT_DOMAIN_COUNT: usize = 76_767;

#[derive(Debug)]
pub struct DomainPolicy {
    domains: HashSet<String>,
}

impl DomainPolicy {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).context("failed to open adult-domain policy")?;
        let mut bytes = Vec::new();
        file.take(MAX_POLICY_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("failed to read adult-domain policy")?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_POLICY_BYTES,
            "adult-domain policy exceeds {MAX_POLICY_BYTES} bytes"
        );
        let contents = std::str::from_utf8(&bytes).context("adult-domain policy is not UTF-8")?;
        let mut domains = HashSet::new();

        for (index, raw_line) in contents.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line
                .split_once('#')
                .map_or(raw_line, |(record, _)| record)
                .trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split_whitespace();
            let address = fields
                .next()
                .expect("a nonempty split always contains one field")
                .parse::<IpAddr>()
                .with_context(|| {
                    format!("adult-domain policy line {line_number} has an invalid address")
                })?;
            anyhow::ensure!(
                address.is_unspecified() || address.is_loopback(),
                "adult-domain policy line {line_number} does not use a null-route address"
            );

            let mut found_domain = false;
            for field in fields {
                found_domain = true;
                let domain = normalize_domain(field).with_context(|| {
                    format!("adult-domain policy line {line_number} has an invalid domain")
                })?;
                domains.insert(domain);
                anyhow::ensure!(
                    domains.len() <= MAX_POLICY_ENTRIES,
                    "adult-domain policy exceeds {MAX_POLICY_ENTRIES} entries"
                );
            }
            anyhow::ensure!(
                found_domain,
                "adult-domain policy line {line_number} has no domains"
            );
        }
        anyhow::ensure!(
            !domains.is_empty(),
            "adult-domain policy contains no domains"
        );
        Ok(Self { domains })
    }

    pub fn contains_host(&self, host: &str) -> bool {
        let Ok(host) = normalize_domain(host) else {
            return false;
        };
        let mut candidate = host.as_str();
        loop {
            if self.domains.contains(candidate) {
                return true;
            }
            let Some(separator) = candidate.find('.') else {
                return false;
            };
            candidate = &candidate[separator + 1..];
        }
    }

    pub fn entry_count(&self) -> usize {
        self.domains.len()
    }
}

fn normalize_domain(domain: &str) -> Result<String> {
    let domain = domain.strip_suffix('.').unwrap_or(domain);
    anyhow::ensure!(
        !domain.is_empty()
            && !domain.ends_with('.')
            && domain.len() <= 253
            && domain.contains('.')
            && domain.parse::<IpAddr>().is_err(),
        "invalid domain"
    );
    for label in domain.split('.') {
        anyhow::ensure!(
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "invalid domain"
        );
    }
    Ok(domain.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::DomainPolicy;

    fn load(contents: &[u8]) -> (TempDir, anyhow::Result<DomainPolicy>) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("adult-domains.hosts");
        fs::write(&path, contents).unwrap();
        let result = DomainPolicy::load(&path);
        (root, result)
    }

    #[test]
    fn parses_hosts_records_and_matches_only_exact_domains_or_subdomains() {
        let (_root, policy) = load(
            b"# source metadata\n\
              0.0.0.0 Example.test\n\
              127.0.0.1 second.example.test third.example.test # inline note\n\
              0.0.0.0 example.test\n",
        );
        let policy = policy.unwrap();

        assert_eq!(policy.entry_count(), 3);
        for blocked in [
            "example.test",
            "EXAMPLE.TEST",
            "example.test.",
            "www.example.test",
            "deep.www.example.test",
            "second.example.test",
            "third.example.test",
        ] {
            assert!(policy.contains_host(blocked), "expected {blocked} to block");
        }
        for allowed in [
            "notexample.test",
            "example.test.example",
            "example.invalid",
            "127.0.0.1",
            "::1",
        ] {
            assert!(!policy.contains_host(allowed), "expected {allowed} to pass");
        }
    }

    #[test]
    fn rejects_missing_empty_malformed_and_non_utf8_inputs() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            DomainPolicy::load(&root.path().join("missing"))
                .unwrap_err()
                .to_string(),
            "failed to open adult-domain policy"
        );

        for (contents, expected) in [
            (
                &b"# comments only\n"[..],
                "adult-domain policy contains no domains",
            ),
            (
                &b"not-an-ip example.test\n"[..],
                "adult-domain policy line 1 has an invalid address",
            ),
            (
                &b"0.0.0.0 bad%domain.test\n"[..],
                "adult-domain policy line 1 has an invalid domain",
            ),
            (
                &b"0.0.0.0 127.0.0.1\n"[..],
                "adult-domain policy line 1 has an invalid domain",
            ),
            (
                &b"0.0.0.0\n"[..],
                "adult-domain policy line 1 has no domains",
            ),
            (&b"\xff\xfe\n"[..], "adult-domain policy is not UTF-8"),
        ] {
            let (_root, result) = load(contents);
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
    }

    #[test]
    fn rejects_non_null_addresses_and_oversized_policy_files() {
        let (_root, result) = load(b"192.0.2.1 example.test\n");
        assert_eq!(
            result.unwrap_err().to_string(),
            "adult-domain policy line 1 does not use a null-route address"
        );

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("oversized.hosts");
        fs::write(&path, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
        assert_eq!(
            DomainPolicy::load(&path).unwrap_err().to_string(),
            "adult-domain policy exceeds 4194304 bytes"
        );
    }

    fn base36(mut value: usize) -> String {
        let mut digits = Vec::new();
        loop {
            let digit = (value % 36) as u8;
            digits.push(if digit < 10 {
                b'0' + digit
            } else {
                b'a' + digit - 10
            });
            value /= 36;
            if value == 0 {
                break;
            }
        }
        digits.reverse();
        String::from_utf8(digits).unwrap()
    }

    #[test]
    fn rejects_more_than_the_bounded_number_of_unique_domains() {
        let mut contents = String::with_capacity(4 * 1024 * 1024);
        for index in 0..=250_000 {
            contents.push_str("0.0.0.0 ");
            contents.push_str(&base36(index));
            contents.push_str(".x\n");
        }
        assert!(contents.len() <= 4 * 1024 * 1024);
        let (_root, result) = load(contents.as_bytes());
        assert_eq!(
            result.unwrap_err().to_string(),
            "adult-domain policy exceeds 250000 entries"
        );
    }

    #[test]
    fn pinned_policy_loads_with_the_reviewed_entry_count() {
        let path = std::env::var_os("OMARCHY_KIDS_BLOCKLIST_PATH")
            .expect("OMARCHY_KIDS_BLOCKLIST_PATH must be supplied by the build environment");
        let policy = DomainPolicy::load(path.as_ref()).unwrap();

        assert_eq!(policy.entry_count(), super::PINNED_ADULT_DOMAIN_COUNT);
    }
}
