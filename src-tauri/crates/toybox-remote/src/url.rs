//! Server addresses: what the user types becomes one normalised base URL.
//!
//! * a missing scheme means `https://`;
//! * `https://` is always accepted; plain `http://` only for this computer
//!   (`localhost`, `127.0.0.1`, `[::1]`) and private networks (`10.x`,
//!   `172.16-31.x`, `192.168.x`, `*.local`), with a plain warning, because the
//!   access token is sent in every request;
//! * no trailing slash, no `/api/v1` suffix, no query or fragment, no password.
//!
//! The same rules are mirrored in `src/domain/remoteUrl.ts` for the dialog.

/// A usable address and, for plain `http`, what to tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerUrl {
    pub url: String,
    pub warning: Option<String>,
}

const LOCAL_WARNING: &str = "This address is not encrypted. That is fine on this computer, but anything that is not on it should use https://.";
const LAN_WARNING: &str = "This address is not encrypted. Anyone on your network could read your access token. Use it only on a network you trust.";

enum Reach {
    ThisComputer,
    PrivateNetwork,
    Public,
}

fn reach(host: &str) -> Reach {
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host == "127.0.0.1" || host == "[::1]" {
        return Reach::ThisComputer;
    }
    if let Some(octets) = ipv4(&host) {
        return match octets {
            [127, ..] => Reach::ThisComputer,
            [10, ..] | [192, 168, ..] => Reach::PrivateNetwork,
            [172, second, ..] if (16..=31).contains(&second) => Reach::PrivateNetwork,
            _ => Reach::Public,
        };
    }
    if host.ends_with(".local") && host.len() > ".local".len() {
        return Reach::PrivateNetwork;
    }
    Reach::Public
}

fn ipv4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (slot, part) in out.iter_mut().zip(&parts) {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    Some(out)
}

fn valid_host(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
    }
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && !host.contains("..")
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// `host` and `:port` of an authority; an IPv6 literal keeps its brackets.
fn split_authority(authority: &str) -> (&str, Option<&str>) {
    if authority.starts_with('[') {
        return match authority.find(']') {
            Some(end) if authority[end + 1..].is_empty() => (authority, None),
            Some(end) => match authority[end + 1..].strip_prefix(':') {
                Some(port) => (&authority[..=end], Some(port)),
                None => (authority, None),
            },
            None => (authority, None),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    }
}

/// Normalise and check what the user typed. The error is a sentence for the dialog.
pub fn normalize_server_url(input: &str) -> Result<ServerUrl, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Enter the address of the server.".into());
    }
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The address cannot contain spaces.".into());
    }
    let (scheme, rest) = match text.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("https".to_string(), text),
    };
    if scheme != "http" && scheme != "https" {
        return Err("The address must start with https:// (or http:// for a server on this computer or your own network).".into());
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return Err("The address cannot contain a user name or password. Enter the access token separately.".into());
    }
    let (host, port) = split_authority(authority);
    if !valid_host(host) {
        return Err("That does not look like a server address.".into());
    }
    if let Some(port) = port {
        match port.parse::<u32>() {
            Ok(1..=65535) => {}
            _ => return Err("The port must be a number between 1 and 65535.".into()),
        }
    }
    let mut path = tail.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').to_string();
    for suffix in ["/api/v1", "/api"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            path = stripped.to_string();
            break;
        }
    }
    let warning = if scheme == "http" {
        match reach(host) {
            Reach::ThisComputer => Some(LOCAL_WARNING.to_string()),
            Reach::PrivateNetwork => Some(LAN_WARNING.to_string()),
            Reach::Public => return Err("Use an https:// address. Plain http:// would send your access token across the internet where anyone can read it.".into()),
        }
    } else {
        None
    };
    let port = port.map(|p| format!(":{p}")).unwrap_or_default();
    Ok(ServerUrl { url: format!("{scheme}://{}{port}{path}", host.to_ascii_lowercase()), warning })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> ServerUrl {
        normalize_server_url(input).unwrap_or_else(|e| panic!("{input}: {e}"))
    }

    #[test]
    fn a_missing_scheme_means_https_and_the_form_is_normalised() {
        assert_eq!(ok("assets.example.com").url, "https://assets.example.com");
        assert_eq!(ok("  Assets.Example.com/  ").url, "https://assets.example.com");
        assert_eq!(ok("https://assets.example.com:8443/toybox/").url, "https://assets.example.com:8443/toybox");
        assert_eq!(ok("https://assets.example.com/api/v1").url, "https://assets.example.com");
        assert_eq!(ok("https://assets.example.com/?x=1#top").url, "https://assets.example.com");
        assert_eq!(ok("HTTPS://a.example.com").warning, None);
        assert_eq!(ok("assets.example.com:8787").url, "https://assets.example.com:8787");
    }

    #[test]
    fn plain_http_only_for_this_computer_and_private_networks_and_with_a_warning() {
        for local in ["http://localhost:8787", "http://127.0.0.1:8787/", "http://[::1]:8787", "localhost:8787"] {
            let url = normalize_server_url(local);
            if local.starts_with("localhost") {
                // No scheme means https, which is allowed anywhere.
                assert_eq!(url.unwrap().url, "https://localhost:8787");
                continue;
            }
            let url = url.unwrap_or_else(|e| panic!("{local}: {e}"));
            assert!(url.url.starts_with("http://"), "{local}");
            assert_eq!(url.warning.as_deref(), Some(LOCAL_WARNING), "{local}");
        }
        for lan in ["http://192.168.1.20:8787", "http://10.0.0.5", "http://172.16.0.1", "http://172.31.255.254", "http://nas.local:8787"] {
            assert_eq!(ok(lan).warning.as_deref(), Some(LAN_WARNING), "{lan}");
        }
        for public in ["http://example.com", "http://8.8.8.8", "http://172.32.0.1", "http://172.15.0.1", "http://192.169.0.1", "http://local", "http://notlocal.localhost.com"] {
            assert!(normalize_server_url(public).is_err(), "{public}");
        }
        assert!(ok("https://192.168.1.20").warning.is_none(), "https never warns");
    }

    #[test]
    fn nonsense_is_refused_with_a_sentence() {
        for bad in ["", "   ", "ftp://example.com", "https://", "https://user:pw@example.com", "https://exa mple.com", "https://example.com:0", "https://example.com:99999", "https://example.com:abc", "https://-bad.example.com", "https://a..b.com", "ws://example.com"] {
            let error = normalize_server_url(bad).unwrap_err();
            assert!(error.ends_with('.'), "{bad}: {error}");
        }
    }

    #[test]
    fn ipv6_literals_and_ports() {
        assert_eq!(ok("https://[2001:db8::1]:8443").url, "https://[2001:db8::1]:8443");
        assert_eq!(ok("http://[::1]").url, "http://[::1]");
        assert!(normalize_server_url("http://[2001:db8::1]").is_err());
    }
}
