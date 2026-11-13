//! The credential boundary for every pull and listing (audit 2026-11-12, item 3).
//!
//! - Each authenticated client is bound to one exact origin (`https://api.notion.com`). Every
//!   URL is checked before a request is built: `https` only, that host exactly, port 443, no
//!   user name or password. A URL a service hands back (Productboard's `links.next`) gets the
//!   same check, so a token is never sent anywhere else.
//! - Clients never follow redirects (`max_redirects(0)`): a redirect is answered as an error,
//!   so no header, key or body is replayed to another host. Google Sheets (no credentials)
//!   follows redirects by hand, within Google's own hosts.
//! - Cursors and ids are percent-encoded into URLs, and every paged loop is bounded and stops
//!   on a repeated cursor.
//! - Every call is recorded in the outbound log (host and purpose only).

use anyhow::{Result, bail};
use std::collections::HashSet;
use std::time::Duration;

/// The most pages one pull or listing reads from one endpoint. A 591k-row collection at 100
/// rows a page is ~6,000 pages; past this a service is looping or misbehaving.
pub(crate) const MAX_PAGES: usize = 50_000;

/// Refuse `url` unless it is exactly on `origin` (`https://host`).
pub(crate) fn check_url(url: &str, origin: &str) -> Result<()> {
    let want = url::Url::parse(origin).expect("valid origin constant");
    let Ok(u) = url::Url::parse(url) else {
        bail!(
            "refusing a malformed URL from {}",
            want.host_str().unwrap_or("?")
        );
    };
    let same = u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.host_str() == want.host_str()
        && u.port_or_known_default() == Some(443);
    if !same {
        bail!(
            "refusing to send credentials to {}://{}: only {origin} is allowed",
            u.scheme(),
            u.host_str().unwrap_or("?")
        );
    }
    Ok(())
}

/// `v` percent-encoded for a query value or a path segment: everything but ASCII letters,
/// digits and `-_.~` is escaped (a space is `%20`, never `+`).
pub(crate) fn enc(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The HTTP client for pulls and listings: no redirects, bounded waits.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        // Our own check enforces https before any request; ureq's flag is a second lock (off
        // under test, where requests go to a local http stand-in after the check).
        .https_only(!cfg!(test))
        .timeout_connect(Some(Duration::from_secs(15)))
        // Shorter under test, so a stalled stand-in proves the deadline in seconds.
        .timeout_recv_response(Some(Duration::from_secs(if cfg!(test) { 2 } else { 60 })))
        .timeout_global(Some(Duration::from_secs(if cfg!(test) { 5 } else { 120 })))
        .build()
        .into()
}

/// The error for a redirect answer: never followed with credentials attached.
pub(crate) fn refuse_redirect(service: &str, status: u16) -> anyhow::Error {
    anyhow::anyhow!(
        "{service} answered with a redirect (HTTP {status}); inVectus does not follow redirects \
         with your token attached"
    )
}

/// Bounds a paged loop: at most [`MAX_PAGES`] pages, and the same cursor never twice.
pub(crate) struct Pages {
    what: &'static str,
    seen: HashSet<String>,
}

impl Pages {
    pub(crate) fn new(what: &'static str) -> Self {
        Self {
            what,
            seen: HashSet::new(),
        }
    }

    /// Call with each cursor (or next-page URL) before fetching it.
    pub(crate) fn next(&mut self, cursor: &str) -> Result<()> {
        if !self.seen.insert(cursor.to_string()) {
            bail!(
                "{} returned the same page cursor twice; stopped to avoid an endless loop",
                self.what
            );
        }
        if self.seen.len() > MAX_PAGES {
            bail!(
                "{} returned more than {MAX_PAGES} pages; stopped",
                self.what
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_https_origin_passes() {
        let o = "https://api.productboard.com";
        assert!(check_url("https://api.productboard.com/notes?pageCursor=x", o).is_ok());
        assert!(check_url("https://api.productboard.com:443/features", o).is_ok());
        for bad in [
            "http://api.productboard.com/notes",
            "https://evil.example/notes",
            "https://api.productboard.com.evil.example/notes",
            "https://api.productboard.com@evil.example/notes",
            "https://user:pw@api.productboard.com/notes",
            "https://api.productboard.com:8443/notes",
            "https://evil.example/https://api.productboard.com/",
            "//evil.example/notes",
            "notes",
            "javascript:alert(1)",
        ] {
            assert!(check_url(bad, o).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn cursors_are_encoded_as_one_value() {
        assert_eq!(enc("a&b=c#d e/f?+"), "a%26b%3Dc%23d%20e%2Ff%3F%2B");
    }

    #[test]
    fn repeated_cursors_and_endless_paging_stop() {
        let mut p = Pages::new("Test");
        p.next("a").unwrap();
        p.next("b").unwrap();
        assert!(
            p.next("a")
                .unwrap_err()
                .to_string()
                .contains("same page cursor")
        );
        let mut p = Pages::new("Test");
        let err = (0..=MAX_PAGES)
            .try_for_each(|i| p.next(&i.to_string()))
            .unwrap_err();
        assert!(err.to_string().contains("more than"));
    }
}
