//! The client's address behind a reverse proxy: upstream's `SetRemoteAddrMiddleware` with the settings of its Docker
//! image. Only when `BEHIND_HTTPS_PROXY` or `BEHIND_PLAIN_HTTP_PROXY` is set is a header trusted: `X-Real-IP` by
//! default, or `X-Forwarded-For` with an exact number of proxies. The address ends up in events (`remote_addr`) and
//! in `{{auto}}` user IPs, so ingest is where it is used.

use std::net::SocketAddr;

use axum::http::HeaderMap;

use crate::alerts::py_bool;

#[derive(clap::Args, Clone, Debug)]
pub struct ProxySettings {
    #[arg(long, env = "BEHIND_HTTPS_PROXY", default_value = "false", value_parser = py_bool)]
    pub behind_https_proxy: bool,
    #[arg(long, env = "BEHIND_PLAIN_HTTP_PROXY", default_value = "false", value_parser = py_bool)]
    pub behind_plain_http_proxy: bool,
    /// Behind a proxy: defaults to true.
    #[arg(long, env = "USE_X_REAL_IP", value_parser = py_bool)]
    pub use_x_real_ip: Option<bool>,
    #[arg(long, env = "USE_X_FORWARDED_FOR", default_value = "false", value_parser = py_bool)]
    pub use_x_forwarded_for: bool,
    /// The number of proxies that append to X-Forwarded-For (needed with USE_X_FORWARDED_FOR).
    #[arg(long, env = "X_FORWARDED_FOR_PROXY_COUNT", default_value_t = 0)]
    pub x_forwarded_for_proxy_count: i64,
}

/// Where the client's address comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClientAddr {
    Peer,
    XRealIp,
    XForwardedFor { proxy_count: usize },
}

impl ProxySettings {
    /// Upstream's settings checks: X-Real-IP and X-Forwarded-For exclude each other, and the latter needs a count.
    pub fn client_addr(&self) -> anyhow::Result<ClientAddr> {
        if !(self.behind_https_proxy || self.behind_plain_http_proxy) {
            return Ok(ClientAddr::Peer);
        }
        let use_x_real_ip = self.use_x_real_ip.unwrap_or(true);
        anyhow::ensure!(
            !(use_x_real_ip && self.use_x_forwarded_for),
            "USE_X_REAL_IP=True and USE_X_FORWARDED_FOR=True are mutually exclusive"
        );
        if self.use_x_forwarded_for {
            anyhow::ensure!(
                self.x_forwarded_for_proxy_count > 0,
                "USE_X_FORWARDED_FOR=True requires X_FORWARDED_FOR_PROXY_COUNT to be set to a positive integer"
            );
            return Ok(ClientAddr::XForwardedFor { proxy_count: self.x_forwarded_for_proxy_count as usize });
        }
        Ok(if use_x_real_ip { ClientAddr::XRealIp } else { ClientAddr::Peer })
    }
}

/// More addresses in X-Forwarded-For than there are proxies: someone added some (upstream: `SuspiciousOperation`,
/// a 400).
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("X-Forwarded-For header does not contain the expected number of addresses")]
pub struct SpoofedForwardedFor;

impl ClientAddr {
    /// The address as upstream's `REMOTE_ADDR`: none when the configured header is missing or empty (a proxy
    /// misconfiguration). Header values are taken as they are, like upstream (an address with a port stays so).
    pub fn of(self, headers: &HeaderMap, peer: SocketAddr) -> Result<Option<String>, SpoofedForwardedFor> {
        match self {
            Self::Peer => Ok(Some(peer.ip().to_string())),
            Self::XRealIp => Ok(header(headers, "x-real-ip")),
            Self::XForwardedFor { proxy_count } => {
                parse_x_forwarded_for(header(headers, "x-forwarded-for").as_deref(), proxy_count)
            }
        }
    }
}

/// A request header as WSGI sees it: repeated headers joined with a comma (gunicorn).
fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    let values: Vec<&str> = headers.get_all(name).iter().filter_map(|v| v.to_str().ok()).collect();
    (!values.is_empty()).then(|| values.join(","))
}

/// `SetRemoteAddrMiddleware.parse_x_forwarded_for`: each proxy appends the address it forwards for, so with N proxies
/// there are exactly N addresses and the first is the client.
fn parse_x_forwarded_for(value: Option<&str>, proxy_count: usize) -> Result<Option<String>, SpoofedForwardedFor> {
    let Some(value) = value.filter(|v| !v.is_empty()) else { return Ok(None) };
    let ips: Vec<&str> = value.split(',').map(crate::grouping::py::strip).collect();
    match ips.len().cmp(&proxy_count) {
        std::cmp::Ordering::Equal => Ok(Some(ips[0].to_owned())),
        std::cmp::Ordering::Greater => Err(SpoofedForwardedFor),
        std::cmp::Ordering::Less => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn settings(https: bool, real_ip: Option<bool>, xff: bool, count: i64) -> ProxySettings {
        ProxySettings {
            behind_https_proxy: https,
            behind_plain_http_proxy: false,
            use_x_real_ip: real_ip,
            use_x_forwarded_for: xff,
            x_forwarded_for_proxy_count: count,
        }
    }

    #[test]
    fn settings_like_upstreams_docker_image() {
        assert_eq!(settings(false, Some(true), true, 1).client_addr().unwrap(), ClientAddr::Peer);
        assert_eq!(settings(true, None, false, 0).client_addr().unwrap(), ClientAddr::XRealIp);
        assert_eq!(settings(true, Some(false), false, 0).client_addr().unwrap(), ClientAddr::Peer);
        assert_eq!(
            settings(true, Some(false), true, 2).client_addr().unwrap(),
            ClientAddr::XForwardedFor { proxy_count: 2 }
        );
        assert!(settings(true, None, true, 1).client_addr().is_err());
        assert!(settings(true, Some(false), true, 0).client_addr().is_err());
    }

    #[test]
    fn parses_x_forwarded_for_like_upstream() {
        // upstream's test_parse_x_forwarded_for_one_proxy
        assert_eq!(parse_x_forwarded_for(None, 1), Ok(None));
        assert_eq!(parse_x_forwarded_for(Some(""), 1), Ok(None));
        assert_eq!(parse_x_forwarded_for(Some("1.2.3.4"), 1), Ok(Some("1.2.3.4".into())));
        assert_eq!(parse_x_forwarded_for(Some("123.123.123.123,1.2.3.4"), 1), Err(SpoofedForwardedFor));
        // two proxies; whitespace around commas; too few is a misconfiguration
        assert_eq!(parse_x_forwarded_for(Some("9.9.9.9 , 10.0.0.1"), 2), Ok(Some("9.9.9.9".into())));
        assert_eq!(parse_x_forwarded_for(Some("9.9.9.9"), 2), Ok(None));
    }

    #[test]
    fn reads_headers() {
        let peer: SocketAddr = "192.0.2.1:5000".parse().unwrap();
        let mut h = HeaderMap::new();
        assert_eq!(ClientAddr::Peer.of(&h, peer), Ok(Some("192.0.2.1".into())));
        assert_eq!(ClientAddr::XRealIp.of(&h, peer), Ok(None));
        h.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));
        assert_eq!(ClientAddr::XRealIp.of(&h, peer), Ok(Some("203.0.113.9".into())));
        h.append("x-forwarded-for", HeaderValue::from_static("1.1.1.1"));
        h.append("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        assert_eq!(ClientAddr::XForwardedFor { proxy_count: 2 }.of(&h, peer), Ok(Some("1.1.1.1".into())));
    }
}
