use crate::error::{VoiceError, VoiceResult};
use crate::idle::IdleTimeout;
use std::io::Read;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
// ureq 3.4.1+ treats the body timeout as a total budget, so it only bounds the helper thread behind
// IdleTimeout; resumed installs make a download cut at this limit cheap to continue.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(3600);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: u32 = 5;

pub struct Response {
    pub status: u16,
    pub content_range_total: Option<u64>,
    pub body: Box<dyn Read>,
}

pub trait Transport: Send + Sync {
    fn get(&self, url: &str, range_from: Option<u64>) -> VoiceResult<Response>;
}

fn bracketed_host(text: &str) -> Option<&str> {
    let (host, after) = text.split_once(']')?;
    (after.is_empty() || after.starts_with(':')).then_some(host)
}

fn host_of(rest: &str) -> Option<&str> {
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.contains('\\'))?;
    let authority = authority.rsplit('@').next()?;
    match authority.strip_prefix('[') {
        Some(bracketed) => bracketed_host(bracketed),
        None => authority.split(':').next(),
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub(crate) fn check_url(url: &str) -> VoiceResult<()> {
    let allowed = url.starts_with("https://")
        || url
            .strip_prefix("http://")
            .and_then(host_of)
            .is_some_and(is_loopback);
    allowed
        .then_some(())
        .ok_or_else(|| VoiceError::Network(format!("refusing non-HTTPS URL: {url}")))
}

pub(crate) fn content_range_total(value: &str) -> Option<u64> {
    value
        .trim()
        .strip_prefix("bytes ")?
        .rsplit('/')
        .next()?
        .parse()
        .ok()
}

fn agent(https_only: bool, max_redirects: u32) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(https_only)
            .http_status_as_error(false)
            .max_redirects(max_redirects)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(TOTAL_TIMEOUT))
            .timeout_recv_body(Some(TOTAL_TIMEOUT))
            .build(),
    )
}

pub struct UreqTransport {
    secure: ureq::Agent,
    loopback: ureq::Agent,
    stall: Duration,
}

impl UreqTransport {
    pub fn new() -> Self {
        Self::with_stall_timeout(STALL_TIMEOUT)
    }

    /// Fails a body read that waits longer than `stall` for the next bytes.
    pub fn with_stall_timeout(stall: Duration) -> Self {
        Self {
            secure: agent(true, MAX_REDIRECTS),
            loopback: agent(false, 0),
            stall,
        }
    }

    fn agent_for(&self, url: &str) -> &ureq::Agent {
        if url.starts_with("https://") {
            &self.secure
        } else {
            &self.loopback
        }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str, range_from: Option<u64>) -> VoiceResult<Response> {
        check_url(url)?;
        let mut request = self
            .agent_for(url)
            .get(url)
            .header("Accept-Encoding", "identity");
        if let Some(from) = range_from {
            request = request.header("Range", format!("bytes={from}-"));
        }
        let response = request
            .call()
            .map_err(|error| VoiceError::Network(error.to_string()))?;
        let total = response
            .headers()
            .get("Content-Range")
            .and_then(|value| value.to_str().ok())
            .and_then(content_range_total);
        Ok(Response {
            status: response.status().as_u16(),
            content_range_total: total,
            body: Box::new(IdleTimeout::new(
                response.into_body().into_reader(),
                self.stall,
            )),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_allowed_everywhere() {
        assert_eq!(check_url("https://huggingface.co/x/voices.json"), Ok(()));
    }

    #[test]
    fn plain_http_is_allowed_only_for_loopback() {
        for url in [
            "http://127.0.0.1:8080/a",
            "http://localhost/a",
            "http://[::1]:9/a",
        ] {
            assert_eq!(check_url(url), Ok(()), "{url}");
        }
        for url in [
            "http://example.com/a",
            "http://127.0.0.1.evil.com/a",
            "http://127.0.0.1@evil.com/a",
            "http://localhost.evil.com/a",
            "http://[::1]evil.com/",
            "http://[::1",
            "http://evil.com\\@127.0.0.1/",
            "ftp://localhost/a",
            "file:///etc/passwd",
            "",
        ] {
            assert!(
                matches!(check_url(url), Err(VoiceError::Network(_))),
                "{url}"
            );
        }
    }

    #[test]
    fn content_range_total_reads_the_size_after_the_slash() {
        assert_eq!(content_range_total("bytes 100-999/1000"), Some(1000));
        assert_eq!(content_range_total(" bytes 0-9/10 "), Some(10));
        assert_eq!(content_range_total("bytes 0-9/*"), None);
        assert_eq!(content_range_total("items 0-9/10"), None);
        assert_eq!(content_range_total(""), None);
    }
}
