//! System proxy detection for `kaku update`-style curl invocations.
//!
//! When Kaku is launched from the menu bar or a notification, the process
//! env inherited from launchd has no proxy vars, so curl cannot reach the
//! internet through a user-configured proxy. This module resolves the
//! macOS system proxy from Network Settings via `/usr/sbin/scutil --proxy`
//! and exposes it for the update code paths in both `kaku` and `kaku-gui`.
//!
//! On non-macOS platforms `scutil` will simply fail to spawn and the
//! detection returns `None`, which is the correct fallback.

use std::process::Command;

const PROXY_ENV_VARS: &[&str] = &[
    "https_proxy",
    "HTTPS_PROXY",
    "http_proxy",
    "HTTP_PROXY",
    "ALL_PROXY",
    "all_proxy",
];

/// Detect the system proxy.
///
/// Returns `None` when any proxy env var is already set (curl picks it up
/// automatically) or when detection is unavailable on this platform.
pub fn detect_system_proxy() -> Option<String> {
    if PROXY_ENV_VARS.iter().any(|v| std::env::var(v).is_ok()) {
        return None;
    }

    let out = Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&out.stdout);
    parse_scutil_proxy(&text)
}

/// Parse `scutil --proxy` output and return the preferred proxy URL
/// (HTTPS > HTTP > SOCKS). Returns `None` when no enabled proxy has both
/// host and port set.
pub fn parse_scutil_proxy(text: &str) -> Option<String> {
    let mut https_enabled = false;
    let mut https_host = String::new();
    let mut https_port = String::new();
    let mut http_enabled = false;
    let mut http_host = String::new();
    let mut http_port = String::new();
    let mut socks_enabled = false;
    let mut socks_host = String::new();
    let mut socks_port = String::new();

    for line in text.lines() {
        if let Some((key, val)) = line.trim().split_once(" : ") {
            match key.trim() {
                "HTTPSEnable" => https_enabled = val.trim() == "1",
                "HTTPSProxy" => https_host = val.trim().to_string(),
                "HTTPSPort" => https_port = val.trim().to_string(),
                "HTTPEnable" => http_enabled = val.trim() == "1",
                "HTTPProxy" => http_host = val.trim().to_string(),
                "HTTPPort" => http_port = val.trim().to_string(),
                "SOCKSEnable" => socks_enabled = val.trim() == "1",
                "SOCKSProxy" => socks_host = val.trim().to_string(),
                "SOCKSPort" => socks_port = val.trim().to_string(),
                _ => {}
            }
        }
    }

    if https_enabled && is_valid_host(&https_host) && is_valid_port(&https_port) {
        return Some(format!("http://{}:{}", https_host, https_port));
    }
    if http_enabled && is_valid_host(&http_host) && is_valid_port(&http_port) {
        return Some(format!("http://{}:{}", http_host, http_port));
    }
    if socks_enabled && is_valid_host(&socks_host) && is_valid_port(&socks_port) {
        return Some(format!("socks5h://{}:{}", socks_host, socks_port));
    }
    None
}

fn is_valid_port(s: &str) -> bool {
    s.parse::<u16>().map(|p| p > 0).unwrap_or(false)
}

fn is_valid_host(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c == '/' || c == ':')
}

/// Apply `proxy` to `cmd` by setting the standard curl proxy env vars.
/// No-op when `proxy` is `None`.
pub fn apply_to_command(cmd: &mut Command, proxy: &Option<String>) {
    if let Some(p) = proxy {
        cmd.env("https_proxy", p)
            .env("HTTPS_PROXY", p)
            .env("http_proxy", p)
            .env("HTTP_PROXY", p)
            .env("all_proxy", p)
            .env("ALL_PROXY", p);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_scutil_proxy;

    #[test]
    fn prefers_https_then_http_then_socks() {
        let text = r#"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 8443
  HTTPSProxy : proxy.example.com
  SOCKSEnable : 1
  SOCKSPort : 1080
  SOCKSProxy : socks.example.com
}
"#;
        assert_eq!(
            parse_scutil_proxy(text).as_deref(),
            Some("http://proxy.example.com:8443")
        );
    }

    #[test]
    fn uses_socks5h_for_socks_proxy() {
        let text = r#"
<dictionary> {
  HTTPEnable : 0
  HTTPSEnable : 0
  SOCKSEnable : 1
  SOCKSPort : 1080
  SOCKSProxy : 127.0.0.1
}
"#;
        assert_eq!(
            parse_scutil_proxy(text).as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn returns_none_when_no_enabled_proxy_has_endpoint() {
        let text = r#"
<dictionary> {
  HTTPEnable : 0
  HTTPSEnable : 0
  SOCKSEnable : 0
}
"#;
        assert_eq!(parse_scutil_proxy(text), None);
    }

    #[test]
    fn rejects_invalid_port() {
        let text = r#"
<dictionary> {
  HTTPSEnable : 1
  HTTPSPort : not-a-number
  HTTPSProxy : proxy.example.com
}
"#;
        assert_eq!(parse_scutil_proxy(text), None);
    }

    #[test]
    fn rejects_host_with_whitespace_or_separator() {
        let text = r#"
<dictionary> {
  HTTPSEnable : 1
  HTTPSPort : 8443
  HTTPSProxy : bad host/value
}
"#;
        assert_eq!(parse_scutil_proxy(text), None);
    }
}
