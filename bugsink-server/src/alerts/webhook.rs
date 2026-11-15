//! Outbound webhook calls for messaging services, with upstream's SSRF protections
//! (`alerts/service_backends/webhook_security.py` and `base.py`): only http(s) URLs of plain ASCII URL characters;
//! the hostname is resolved on every send and checked against the allow/deny lists and (by default) against
//! non-global addresses; the request then goes to that checked address, without following redirects.

use std::collections::HashSet;
use std::net::{
  IpAddr,
  Ipv4Addr,
  Ipv6Addr,
  SocketAddr,
};
use std::time::Duration;

use url::{
  Host,
  Url,
};

/// Upstream's `ALERTS_WEBHOOK_*` settings.
#[derive(clap::Args, Clone, Debug)]
pub struct WebhookSettings {
  /// `open`, or `allowlist_only` to only allow destinations in ALERTS_WEBHOOK_ALLOW_LIST.
  #[arg(long, env = "ALERTS_WEBHOOK_OUTBOUND_MODE", default_value = "open", value_parser = parse_mode)]
  pub alerts_webhook_outbound_mode: OutboundMode,
  /// Comma-separated hostnames, IPs and CIDR networks.
  #[arg(long, env = "ALERTS_WEBHOOK_ALLOW_LIST", default_value = "", value_parser = parse_destinations)]
  pub alerts_webhook_allow_list: Destinations,
  #[arg(long, env = "ALERTS_WEBHOOK_DENY_LIST", default_value = "", value_parser = parse_destinations)]
  pub alerts_webhook_deny_list: Destinations,
  /// Refuse destinations that resolve to non-global (private, loopback, ...) addresses.
  #[arg(long, env = "ALERTS_WEBHOOK_DENY_NON_GLOBAL", default_value = "true", value_parser = crate::py::py_bool)]
  pub alerts_webhook_deny_non_global: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutboundMode {
  Open,
  AllowlistOnly,
}

impl OutboundMode {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Open => "open",
      Self::AllowlistOnly => "allowlist_only",
    }
  }
}

fn parse_mode(s: &str) -> Result<OutboundMode, String> {
  match s.to_lowercase().as_str() {
    "open" => Ok(OutboundMode::Open),
    "allowlist_only" => Ok(OutboundMode::AllowlistOnly),
    _ => Err("must be one of: open, allowlist_only".into()),
  }
}

#[derive(Clone, Debug, Default)]
pub struct Destinations {
  /// As configured (the settings page shows them).
  pub entries: Vec<String>,
  hosts: HashSet<String>,
  networks: Vec<Network>,
}

/// `_parse_hosts_and_networks`; upstream does this on every send, we once at startup.
fn parse_destinations(s: &str) -> Result<Destinations, String> {
  let mut d = Destinations::default();
  for entry in s.split(',').map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()) {
    if entry.contains("://") {
      return Err(format!("invalid entry: {entry} (use hostname/IP/CIDR, not full URLs)"));
    }
    d.entries.push(entry.clone());
    match Network::parse(&entry) {
      Some(n) => d.networks.push(n),
      None if entry.contains('/') => return Err(format!("invalid entry: {entry}")),
      None => {
        d.hosts.insert(entry);
      }
    }
  }
  Ok(d)
}

impl Destinations {
  fn matches(&self, hostname: &str, ips: &[IpAddr]) -> bool {
    self.hosts.contains(hostname) || ips.iter().any(|ip| self.networks.iter().any(|n| n.contains(*ip)))
  }
}

/// `ipaddress.ip_network(value, strict=False)`.
#[derive(Clone, Copy, Debug)]
struct Network {
  addr: IpAddr,
  prefix: u8,
}

impl Network {
  fn parse(s: &str) -> Option<Self> {
    let (addr, prefix) = match s.split_once('/') {
      Some((a, p)) => (a.parse::<IpAddr>().ok()?, Some(p.parse::<u8>().ok()?)),
      None => (s.parse::<IpAddr>().ok()?, None),
    };
    let max = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.unwrap_or(max);
    (prefix <= max).then_some(Self { addr, prefix })
  }

  const fn v4(a: [u8; 4], prefix: u8) -> Self {
    Self {
      addr: IpAddr::V4(Ipv4Addr::new(a[0], a[1], a[2], a[3])),
      prefix,
    }
  }

  const fn v6(a: [u16; 8], prefix: u8) -> Self {
    Self {
      addr: IpAddr::V6(Ipv6Addr::new(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7])),
      prefix,
    }
  }

  /// Like Python, an address of the other IP version is never in the network.
  fn contains(&self, ip: IpAddr) -> bool {
    match (self.addr, ip) {
      (IpAddr::V4(n), IpAddr::V4(a)) => {
        masked(u32::from(n).into(), 32, self.prefix) == masked(u32::from(a).into(), 32, self.prefix)
      }
      (IpAddr::V6(n), IpAddr::V6(a)) => masked(n.into(), 128, self.prefix) == masked(a.into(), 128, self.prefix),
      _ => false,
    }
  }
}

fn masked(bits: u128, width: u8, prefix: u8) -> u128 {
  if prefix == 0 { 0 } else { bits >> (width - prefix) }
}

// Python 3.12's `ipaddress` tables (the version in the upstream 2.6.3 image).
const V4_PRIVATE: &[Network] = &[
  Network::v4([0, 0, 0, 0], 8),
  Network::v4([10, 0, 0, 0], 8),
  Network::v4([127, 0, 0, 0], 8),
  Network::v4([169, 254, 0, 0], 16),
  Network::v4([172, 16, 0, 0], 12),
  Network::v4([192, 0, 0, 0], 24),
  Network::v4([192, 0, 0, 170], 31),
  Network::v4([192, 0, 2, 0], 24),
  Network::v4([192, 168, 0, 0], 16),
  Network::v4([198, 18, 0, 0], 15),
  Network::v4([198, 51, 100, 0], 24),
  Network::v4([203, 0, 113, 0], 24),
  Network::v4([240, 0, 0, 0], 4),
  Network::v4([255, 255, 255, 255], 32),
];
const V4_PRIVATE_EXCEPTIONS: &[Network] = &[Network::v4([192, 0, 0, 9], 32), Network::v4([192, 0, 0, 10], 32)];
const V4_SHARED: Network = Network::v4([100, 64, 0, 0], 10);
const V6_PRIVATE: &[Network] = &[
  Network::v6([0, 0, 0, 0, 0, 0, 0, 1], 128),
  Network::v6([0, 0, 0, 0, 0, 0, 0, 0], 128),
  Network::v6([0, 0, 0, 0, 0, 0xffff, 0, 0], 96),
  Network::v6([0x64, 0xff9b, 1, 0, 0, 0, 0, 0], 48),
  Network::v6([0x100, 0, 0, 0, 0, 0, 0, 0], 64),
  Network::v6([0x2001, 0, 0, 0, 0, 0, 0, 0], 23),
  Network::v6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0], 32),
  Network::v6([0x2002, 0, 0, 0, 0, 0, 0, 0], 16),
  Network::v6([0x3fff, 0, 0, 0, 0, 0, 0, 0], 20),
  Network::v6([0xfc00, 0, 0, 0, 0, 0, 0, 0], 7),
  Network::v6([0xfe80, 0, 0, 0, 0, 0, 0, 0], 10),
];
const V6_PRIVATE_EXCEPTIONS: &[Network] = &[
  Network::v6([0x2001, 1, 0, 0, 0, 0, 0, 1], 128),
  Network::v6([0x2001, 1, 0, 0, 0, 0, 0, 2], 128),
  Network::v6([0x2001, 3, 0, 0, 0, 0, 0, 0], 32),
  Network::v6([0x2001, 4, 0x112, 0, 0, 0, 0, 0], 48),
  Network::v6([0x2001, 0x20, 0, 0, 0, 0, 0, 0], 28),
  Network::v6([0x2001, 0x30, 0, 0, 0, 0, 0, 0], 28),
];
const NAT64: Network = Network::v6([0x64, 0xff9b, 0, 0, 0, 0, 0, 0], 96);

fn in_any(ip: IpAddr, networks: &[Network]) -> bool {
  networks.iter().any(|n| n.contains(ip))
}

/// Python's `ip.is_global`.
fn py_is_global(ip: IpAddr) -> bool {
  match ip {
    IpAddr::V4(v4) => {
      let ip = IpAddr::V4(v4);
      !V4_SHARED.contains(ip) && !(in_any(ip, V4_PRIVATE) && !in_any(ip, V4_PRIVATE_EXCEPTIONS))
    }
    IpAddr::V6(v6) => {
      match ipv4_mapped(v6) {
        Some(v4) => py_is_global(IpAddr::V4(v4)),
        None => {
          let ip = IpAddr::V6(v6);
          !(in_any(ip, V6_PRIVATE) && !in_any(ip, V6_PRIVATE_EXCEPTIONS))
        }
      }
    }
  }
}

fn ipv4_mapped(ip: Ipv6Addr) -> Option<Ipv4Addr> {
  let bits = u128::from(ip);
  (bits >> 32 == 0xffff).then(|| Ipv4Addr::from(bits as u32))
}

/// IPv4 addresses embedded in an IPv6 one (mapped, 6to4, Teredo, NAT64), which must be global too.
fn embedded_ipv4(ip: IpAddr) -> Vec<Ipv4Addr> {
  let IpAddr::V6(v6) = ip else { return vec![] };
  let bits = u128::from(v6);
  let mut out: Vec<Ipv4Addr> = ipv4_mapped(v6).into_iter().collect();
  if bits >> 112 == 0x2002 {
    out.push(Ipv4Addr::from((bits >> 80) as u32));
  }
  if bits >> 96 == 0x2001_0000 {
    out.push(Ipv4Addr::from((bits >> 64) as u32));
    out.push(Ipv4Addr::from(!(bits as u32)));
  }
  if NAT64.contains(ip) {
    out.push(Ipv4Addr::from(bits as u32));
  }
  out
}

fn is_global(ip: IpAddr) -> bool {
  py_is_global(ip) && embedded_ipv4(ip).into_iter().all(|v4| py_is_global(IpAddr::V4(v4)))
}

/// A failed send, stored on the messaging service's row (`_store_failure_info`). `error_type` uses the names of the
/// exceptions upstream would raise, so the UI reads the same for both.
#[derive(Debug)]
pub struct Failure {
  pub error_type: &'static str,
  pub message: String,
  pub response: Option<(u16, String)>,
}

impl Failure {
  fn value_error(message: impl Into<String>) -> Self {
    Self {
      error_type: "ValueError",
      message: message.into(),
      response: None,
    }
  }

  /// A missing key in the service's config.
  pub fn key_error(key: &str) -> Self {
    Self {
      error_type: "KeyError",
      message: format!("'{key}'"),
      response: None,
    }
  }
}

fn is_url_char(c: char) -> bool {
  c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// Parses and checks a webhook URL without resolving it (`parse_webhook_url`).
fn parse_webhook_url(webhook_url: &str) -> Result<Url, Failure> {
  if !webhook_url.chars().all(is_url_char) {
    return Err(Failure::value_error(
      "Webhook URL must contain only ASCII URL characters.",
    ));
  }
  let url = Url::parse(webhook_url).map_err(|_| Failure::value_error("Webhook URL is malformed."))?;
  if !["http", "https"].contains(&url.scheme()) {
    return Err(Failure::value_error("Webhook URL must use http:// or https://."));
  }
  if url.host().is_none() {
    return Err(Failure::value_error("Webhook URL must include a hostname."));
  }
  Ok(url)
}

/// `validate_webhook_destination`: resolves the URL's host and returns its addresses if sending there is allowed.
async fn validate_destination(settings: &WebhookSettings, url: &Url) -> Result<Vec<IpAddr>, Failure> {
  let hostname = url.host_str().expect("checked").to_lowercase();
  let unresolved = || Failure::value_error(format!("Webhook hostname could not be resolved: {hostname}"));
  let ips: Vec<IpAddr> = match url.host().expect("checked") {
    Host::Ipv4(ip) => vec![ip.into()],
    Host::Ipv6(ip) => vec![ip.into()],
    Host::Domain(domain) => {
      tokio::net::lookup_host((domain, 0))
        .await
        .map_err(|_| unresolved())?
        .map(|addr| addr.ip())
        .collect()
    }
  };
  if ips.is_empty() {
    return Err(unresolved());
  }

  if settings.alerts_webhook_outbound_mode == OutboundMode::AllowlistOnly
    && !settings.alerts_webhook_allow_list.matches(&hostname, &ips)
  {
    return Err(Failure::value_error(format!(
      "Webhook target {hostname} is not allowlisted in ALERTS_WEBHOOK_ALLOW_LIST (mode=allowlist_only)."
    )));
  }
  if settings.alerts_webhook_deny_list.matches(&hostname, &ips) {
    return Err(Failure::value_error(format!(
      "Webhook target {hostname} matches ALERTS_WEBHOOK_DENY_LIST."
    )));
  }
  if settings.alerts_webhook_deny_non_global
    && let Some(ip) = ips.iter().find(|ip| !is_global(**ip))
  {
    return Err(Failure::value_error(format!(
      "Webhook target resolves to non-global IP address {ip}. If this destination is intentional, add it to \
             ALERTS_WEBHOOK_ALLOW_LIST."
    )));
  }
  Ok(ips)
}

/// `validate_webhook_url`, as the config forms use it: the URL's checks and its destination's (resolving the host).
pub async fn validate_webhook_url(settings: &WebhookSettings, webhook_url: &str) -> Result<(), String> {
  let url = parse_webhook_url(webhook_url).map_err(|f| f.message)?;
  validate_destination(settings, &url)
    .await
    .map(|_| ())
    .map_err(|f| f.message)
}

/// `BaseWebhookBackend.safe_post` + `raise_for_status()`: POSTs a JSON body to the checked address.
pub async fn post_json(settings: &WebhookSettings, webhook_url: &str, body: String) -> Result<(), Failure> {
  let url = parse_webhook_url(webhook_url)?;
  let ips = validate_destination(settings, &url).await?;

  let mut client = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .connect_timeout(Duration::from_secs(5))
    .read_timeout(Duration::from_secs(5));
  if let Some(Host::Domain(domain)) = url.host() {
    // Connect to the address we checked, keeping the hostname for Host, SNI and certificate checks. Port 0: the
    // URL's port is used.
    client = client.resolve(domain, SocketAddr::new(ips[0], 0));
  }
  let client = client.build().map_err(|e| {
    Failure {
      error_type: "RequestException",
      message: e.to_string(),
      response: None,
    }
  })?;

  let response = client
    .post(url.clone())
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(body)
    .send()
    .await
    .map_err(|e| {
      Failure {
        error_type: if e.is_timeout() { "Timeout" } else { "ConnectionError" },
        message: e.to_string(),
        response: None,
      }
    })?;

  let status = response.status();
  if status.is_client_error() || status.is_server_error() {
    let side = if status.is_client_error() { "Client" } else { "Server" };
    let message = format!(
      "{} {side} Error: {} for url: {url}",
      status.as_u16(),
      status.canonical_reason().unwrap_or("")
    );
    let text = response.text().await.unwrap_or_default();
    return Err(Failure {
      error_type: "HTTPError",
      message,
      response: Some((status.as_u16(), text)),
    });
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn global(s: &str) -> bool {
    is_global(s.parse().unwrap())
  }

  #[test]
  fn globalness_like_python() {
    // Expected values from Python 3.12's ipaddress (plus upstream's embedded-IPv4 check).
    for (ip, expected) in [
      ("8.8.8.8", true),
      ("10.1.2.3", false),
      ("100.64.0.1", false),
      ("127.0.0.1", false),
      ("169.254.1.1", false),
      ("192.0.0.9", true),
      ("192.0.0.8", false),
      ("224.0.0.1", true),
      ("255.255.255.255", false),
      ("::1", false),
      ("::ffff:8.8.8.8", true),
      ("::ffff:10.0.0.1", false),
      ("2606:4700::1", true),
      ("fd00::1", false),
      ("2002:0a00:0001::", false),   // 6to4: private (and 2002::/16 itself)
      ("64:ff9b::0a00:0001", false), // NAT64 of 10.0.0.1
      ("64:ff9b::0808:0808", true),  // NAT64 of 8.8.8.8
      ("2001:20::1", true),          // exception inside 2001::/23, but Teredo prefix is 2001:0::/32
    ] {
      assert_eq!(global(ip), expected, "{ip}");
    }
  }

  #[test]
  fn destinations() {
    let d = parse_destinations(" Hooks.Example.com , 10.0.0.0/8,::1 ").unwrap();
    assert!(d.matches("hooks.example.com", &[]));
    assert!(d.matches("x", &["10.9.9.9".parse().unwrap()]));
    assert!(d.matches("x", &["::1".parse().unwrap()]));
    assert!(!d.matches("x", &["11.0.0.1".parse().unwrap()]));
    assert!(parse_destinations("https://x").is_err());
    assert!(parse_destinations("x/8").is_err());
  }

  #[test]
  fn url_checks() {
    assert!(parse_webhook_url("https://hooks.slack.com/services/T/B/x").is_ok());
    assert_eq!(
      parse_webhook_url("ftp://x/").unwrap_err().message,
      "Webhook URL must use http:// or https://."
    );
    assert_eq!(
      parse_webhook_url("https://exämple.com/").unwrap_err().message,
      "Webhook URL must contain only ASCII URL characters."
    );
  }
}
