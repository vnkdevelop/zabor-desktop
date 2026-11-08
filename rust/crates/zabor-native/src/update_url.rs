use crate::update_helper::Result;
use reqwest::header::{HeaderMap, LOCATION};
use reqwest::{StatusCode, Url};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) const MAX_REDIRECTS: usize = 4;
pub(super) const MAX_LOCATION_BYTES: usize = 8192;

enum Scope {
    Native { initial: String },
    Sandbox { authority: String },
}

pub(super) struct RedirectChain {
    scope: Scope,
    current: Url,
    visited: BTreeSet<[u8; 32]>,
    redirects: usize,
}

impl RedirectChain {
    pub(super) fn start(raw: &str, version: &str, package: &str, sandbox: bool) -> Result<Self> {
        if !clean_text(raw) || raw.len() > MAX_LOCATION_BYTES {
            return Err("update-url-policy-rejected".into());
        }
        let current = Url::parse(raw).map_err(|_| "update-url-policy-rejected")?;
        let scope = if sandbox {
            let port = current
                .port()
                .filter(|port| *port != 0)
                .ok_or("sandbox-port-required")?;
            let expected = format!("127.0.0.1:{port}");
            if authority(raw) != Some(expected.as_str())
                || current.scheme() != "http"
                || current.host_str() != Some("127.0.0.1")
                || current.query().is_some()
            {
                return Err("sandbox-url-policy-rejected".into());
            }
            Scope::Sandbox {
                authority: expected,
            }
        } else {
            let expected = format!("https://github.com/vnkdevelop/zabor-desktop/releases/download/v{version}/{package}");
            if raw != expected || package != format!("ZABOR-Native-Beta-{version}-x64.zip") {
                return Err("signed-release-route-mismatch".into());
            }
            Scope::Native { initial: expected }
        };
        let mut chain = Self {
            scope,
            current,
            visited: BTreeSet::new(),
            redirects: 0,
        };
        chain.validate(&chain.current, raw)?;
        chain.visited.insert(fingerprint(&chain.current));
        Ok(chain)
    }

    pub(super) fn request_url(&self) -> Url {
        self.current.clone()
    }

    pub(super) fn advance(&mut self, status: StatusCode, headers: &HeaderMap) -> Result<()> {
        if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Err("unsupported-update-redirect".into());
        }
        if self.redirects >= MAX_REDIRECTS {
            return Err("update-redirect-limit".into());
        }
        let mut values = headers.get_all(LOCATION).iter();
        let value = values.next().ok_or("update-redirect-location-missing")?;
        if values.next().is_some()
            || value.as_bytes().is_empty()
            || value.as_bytes().len() > MAX_LOCATION_BYTES
        {
            return Err("update-redirect-location-invalid".into());
        }
        let raw = value
            .to_str()
            .map_err(|_| "update-redirect-location-invalid")?;
        if !clean_text(raw) || raw.starts_with("//") {
            return Err("update-redirect-location-invalid".into());
        }
        let absolute = Url::parse(raw).ok();
        let next = if let Some(next) = absolute {
            self.validate(&next, raw)?;
            next
        } else {
            let next = self
                .current
                .join(raw)
                .map_err(|_| "update-relative-location-invalid")?;
            if next.origin() != self.current.origin() {
                return Err("update-relative-origin-change".into());
            }
            self.validate(&next, next.as_str())?;
            next
        };
        let identity = fingerprint(&next);
        if !self.visited.insert(identity) {
            return Err("update-redirect-loop".into());
        }
        self.current = next;
        self.redirects += 1;
        Ok(())
    }

    fn validate(&self, url: &Url, raw: &str) -> Result<()> {
        if url.as_str().len() > MAX_LOCATION_BYTES
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.path().is_empty()
            || url.path() == "/"
        {
            return Err("update-url-policy-rejected".into());
        }
        let host = url.host_str().ok_or("update-host-missing")?;
        let raw_authority = authority(raw).ok_or("update-authority-invalid")?;
        if raw_authority.contains('@') {
            return Err("update-userinfo-rejected".into());
        }
        match &self.scope {
            Scope::Native { initial } => {
                if url.scheme() != "https"
                    || !raw.starts_with("https://")
                    || url.port().is_some()
                    || raw_authority.contains(':')
                    || raw_authority.to_ascii_lowercase() != host
                {
                    return Err("native-update-transport-rejected".into());
                }
                match host {
                    "github.com" if url.as_str() == initial && url.query().is_none() => Ok(()),
                    "release-assets.githubusercontent.com" => Ok(()),
                    _ => Err("native-update-host-or-route-rejected".into()),
                }
            }
            Scope::Sandbox {
                authority: expected,
            } => {
                if url.scheme() == "http"
                    && raw.starts_with("http://")
                    && host == "127.0.0.1"
                    && raw_authority == expected
                {
                    Ok(())
                } else {
                    Err("sandbox-redirect-origin-rejected".into())
                }
            }
        }
    }
}

fn clean_text(value: &str) -> bool {
    !value.is_empty()
        && value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte <= 32 || byte == 127 || byte == b'\\')
}

fn authority(raw: &str) -> Option<&str> {
    let (_, rest) = raw.split_once("://")?;
    rest.split(['/', '?', '#']).next()
}

fn fingerprint(url: &Url) -> [u8; 32] {
    Sha256::digest(url.as_str().as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn native() -> RedirectChain {
        RedirectChain::start("https://github.com/vnkdevelop/zabor-desktop/releases/download/v3.8.0-rust/ZABOR-Native-Beta-3.8.0-rust-x64.zip",
            "3.8.0-rust", "ZABOR-Native-Beta-3.8.0-rust-x64.zip", false).unwrap()
    }

    fn location(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn exact_signed_initial_route_rejects_decorations_and_other_targets() {
        let initial = native().request_url().to_string();
        for raw in [
            initial.replace("https:", "http:"),
            initial.replace("github.com/", "github.com:443/"),
            initial.replace("github.com/", "@github.com/"),
            format!("{initial}?sig=LOCAL_FIXTURE"),
            format!("{initial}#x"),
            initial.replace("v3.8.0-rust/", "v3.8.0-rust.1/"),
            initial.replace("github.com/", "github.com.evil.invalid/"),
        ] {
            assert!(RedirectChain::start(
                &raw,
                "3.8.0-rust",
                "ZABOR-Native-Beta-3.8.0-rust-x64.zip",
                false
            )
            .is_err());
        }
    }

    #[test]
    fn only_explicit_https_cdn_is_trusted_and_query_stays_opaque() {
        let mut chain = native();
        chain.advance(StatusCode::FOUND, &location("https://release-assets.githubusercontent.com/asset/path?sig=LOCAL%2BFIXTURE&jwt=LOCAL_FIXTURE")).unwrap();
        assert_eq!(
            chain.request_url().query(),
            Some("sig=LOCAL%2BFIXTURE&jwt=LOCAL_FIXTURE")
        );
        chain
            .advance(
                StatusCode::TEMPORARY_REDIRECT,
                &location("?sig=SECOND_LOCAL_FIXTURE"),
            )
            .unwrap();
        assert_eq!(
            chain.request_url().query(),
            Some("sig=SECOND_LOCAL_FIXTURE")
        );
        chain
            .advance(
                StatusCode::SEE_OTHER,
                &location("../final?sig=THIRD_LOCAL_FIXTURE"),
            )
            .unwrap();
        assert_eq!(
            chain.request_url().host_str(),
            Some("release-assets.githubusercontent.com")
        );
    }

    #[test]
    fn relative_resolution_preserves_origin_and_bounds_resolved_url() {
        let mut chain = native();
        chain.advance(StatusCode::FOUND, &location("https://release-assets.githubusercontent.com/assets/nested/start?sig=LOCAL_FIXTURE")).unwrap();
        chain
            .advance(
                StatusCode::MOVED_PERMANENTLY,
                &location("../final?sig=LOCAL_NEXT"),
            )
            .unwrap();
        assert_eq!(chain.request_url().path(), "/assets/final");
        assert_eq!(chain.request_url().query(), Some("sig=LOCAL_NEXT"));
        let current = chain.request_url();
        for value in [
            "//evil.invalid/asset",
            "///evil.invalid/asset",
            "https:/evil.invalid/asset",
            "#fragment",
            "./final?sig=LOCAL_NEXT",
        ] {
            assert!(chain.advance(StatusCode::FOUND, &location(value)).is_err());
            assert_eq!(chain.request_url(), current);
        }
        let long_path = format!(
            "https://release-assets.githubusercontent.com/{}/start",
            "x".repeat(MAX_LOCATION_BYTES - 100)
        );
        let mut chain = native();
        chain
            .advance(StatusCode::FOUND, &location(&long_path))
            .unwrap();
        assert!(chain
            .advance(StatusCode::FOUND, &location(&"y".repeat(200)))
            .is_err());
        assert_eq!(chain.request_url().as_str(), long_path);
    }

    #[test]
    fn foreign_hosts_ports_userinfo_fragments_and_downgrades_are_rejected() {
        for raw in ["http://release-assets.githubusercontent.com/asset?sig=LOCAL_SECRET",
            "https://release-assets.githubusercontent.com:443/asset", "https://release-assets.githubusercontent.com:8443/asset",
            "https://@release-assets.githubusercontent.com/asset", "https://user:pass@release-assets.githubusercontent.com/asset",
            "https://release-assets.githubusercontent.com/asset#x", "https://release-assets.githubusercontent.com.evil.invalid/asset",
            "https://objects.githubusercontent.com/asset", "https://evil.githubusercontent.com/asset",
            "https://release-assets.githubusercontent.com./asset", "https://127.0.0.1/asset", "https://192.168.1.1/asset",
            "//release-assets.githubusercontent.com/asset", "https://github.com/another/release",
            "https://github.com/vnkdevelop/zabor-desktop/releases/download/v3.8.0-rust/ZABOR-Native-Beta-3.8.0-rust-x64.zip?sig=LOCAL_SECRET"] {
            let error = native().advance(StatusCode::FOUND,&location(raw)).unwrap_err();
            assert!(!error.contains("LOCAL_SECRET")); assert!(!error.contains(raw));
        }
    }

    #[test]
    fn redirect_limit_loop_and_missing_duplicate_or_invalid_location_fail_closed() {
        let mut chain = native();
        let initial = chain.request_url().to_string();
        assert!(chain
            .advance(StatusCode::FOUND, &location(&initial))
            .is_err());
        for status in [
            StatusCode::MULTIPLE_CHOICES,
            StatusCode::NOT_MODIFIED,
            StatusCode::USE_PROXY,
        ] {
            assert!(native()
                .advance(
                    status,
                    &location("https://release-assets.githubusercontent.com/asset")
                )
                .is_err());
        }
        assert!(native()
            .advance(StatusCode::FOUND, &HeaderMap::new())
            .is_err());
        for value in [
            "",
            " ",
            "https://release-assets.githubusercontent.com/has space",
            "https:\\release-assets.githubusercontent.com/asset",
        ] {
            assert!(native()
                .advance(StatusCode::FOUND, &location(value))
                .is_err());
        }
        let mut headers = location("https://release-assets.githubusercontent.com/asset");
        headers.append(
            LOCATION,
            HeaderValue::from_static("https://release-assets.githubusercontent.com/other"),
        );
        assert!(native().advance(StatusCode::FOUND, &headers).is_err());
        headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_bytes(&[0xff]).unwrap());
        assert!(native().advance(StatusCode::FOUND, &headers).is_err());
        assert!(native()
            .advance(
                StatusCode::FOUND,
                &location(&format!(
                    "https://release-assets.githubusercontent.com/a?sig={}",
                    "x".repeat(MAX_LOCATION_BYTES)
                ))
            )
            .is_err());
        let mut chain = native();
        for index in 0..MAX_REDIRECTS {
            chain
                .advance(
                    StatusCode::FOUND,
                    &location(&format!(
                        "https://release-assets.githubusercontent.com/asset?sig=LOCAL_{index}"
                    )),
                )
                .unwrap();
        }
        assert_eq!(
            chain
                .advance(
                    StatusCode::FOUND,
                    &location("https://release-assets.githubusercontent.com/final")
                )
                .unwrap_err(),
            "update-redirect-limit"
        );
    }

    #[test]
    fn sandbox_is_same_explicit_loopback_origin_and_cannot_escalate_scope() {
        for raw in [
            "http://127.1:1234/a",
            "http://localhost:1234/a",
            "http://127.0.0.1/a",
            "http://@127.0.0.1:1234/a",
            "http://127.0.0.1:1234/a?sig=LOCAL",
        ] {
            assert!(RedirectChain::start(
                raw,
                "3.8.0-rust",
                "ZABOR-Native-Beta-3.8.0-rust-x64.zip",
                true
            )
            .is_err());
        }
        for raw in ["http://127.0.0.1:4321/a","http://127.0.0.2:1234/a","http://localhost:1234/a",
            "http://10.0.0.1:1234/a","https://127.0.0.1:1234/a","https://release-assets.githubusercontent.com/asset?sig=LOCAL",
            "https://github.com/vnkdevelop/zabor-desktop/releases/download/v3.8.0-rust/ZABOR-Native-Beta-3.8.0-rust-x64.zip"] {
            let mut chain=RedirectChain::start("http://127.0.0.1:1234/start","3.8.0-rust","ZABOR-Native-Beta-3.8.0-rust-x64.zip",true).unwrap();
            assert!(chain.advance(StatusCode::FOUND,&location(raw)).is_err());
        }
        let mut chain = RedirectChain::start(
            "http://127.0.0.1:1234/start",
            "3.8.0-rust",
            "ZABOR-Native-Beta-3.8.0-rust-x64.zip",
            true,
        )
        .unwrap();
        chain
            .advance(StatusCode::FOUND, &location("/final?sig=LOCAL_FIXTURE"))
            .unwrap();
        assert_eq!(chain.request_url().port(), Some(1234));
    }
}
