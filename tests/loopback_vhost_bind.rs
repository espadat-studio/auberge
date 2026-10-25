//! A Caddy site addressed as `localhost` or `127.0.0.1` is a Host matcher, not
//! a listener: without `bind`, Caddy listens on the port on every interface and
//! serves whoever sends that Host header. A loopback-addressed site therefore
//! has to bind loopback addresses only.

mod common;

use common::caddy::{Vhost, vhosts};

const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];
const LOOPBACK_BINDS: &[&str] = &["127.0.0.1", "[::1]"];

/// The hosts of a site address line such as `http://localhost:8085 {`.
fn site_hosts(line: &str) -> Vec<&str> {
    let Some(addresses) = line.trim_end().strip_suffix('{') else {
        return Vec::new();
    };
    addresses
        .split([',', ' '])
        .filter(|address| !address.is_empty())
        .map(|address| {
            let rest = address
                .strip_prefix("http://")
                .or_else(|| address.strip_prefix("https://"))
                .unwrap_or(address);
            match rest.find(']') {
                Some(end) if rest.starts_with('[') => &rest[..=end],
                _ => rest.split(':').next().unwrap_or_default(),
            }
        })
        .collect()
}

/// Each loopback-addressed site block in one vhost, as its address line and
/// the addresses its `bind` lines name.
fn loopback_sites(vhost: &Vhost) -> Vec<(String, Vec<String>)> {
    let mut sites = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in vhost.body().lines() {
        if !line.starts_with(char::is_whitespace) {
            sites.extend(current.take());
            if site_hosts(line)
                .iter()
                .any(|host| LOOPBACK_HOSTS.contains(host))
            {
                current = Some((format!("{}: {}", vhost.id(), line.trim()), Vec::new()));
            }
            continue;
        }
        if let Some((_, binds)) = current.as_mut()
            && let Some(addresses) = line.trim().strip_prefix("bind ")
        {
            binds.extend(addresses.split_whitespace().map(str::to_string));
        }
    }
    sites.extend(current);
    sites
}

#[test]
fn test_a_loopback_addressed_site_binds_loopback_only() {
    let sites: Vec<_> = vhosts().iter().flat_map(loopback_sites).collect();
    assert!(
        sites
            .iter()
            .any(|(label, _)| label.starts_with("colporteur/Caddyfile-internal.j2")),
        "the scan must reach colporteur's internal vhost: {sites:?}"
    );
    let offenders: Vec<_> = sites
        .iter()
        .filter(|(_, binds)| {
            binds.is_empty() || binds.iter().any(|b| !LOOPBACK_BINDS.contains(&b.as_str()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "loopback-addressed sites must `bind` loopback addresses only: {offenders:?}"
    );
}
