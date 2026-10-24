//! Caddy's global options block: the admin endpoint is a unix socket only the
//! caddy user can open, and nothing turns on debug logging.
//!
//! Left unset, `admin` listens on `localhost:2019` with no authentication, so
//! any local user on the Host can rewrite the running config or read the
//! process environment, which holds every Zone's DNS token. `caddy reload`
//! (the unit's `ExecReload`) runs as the caddy user and reaches the socket by
//! reading the same config, so reload keeps working.
//!
//! `debug`, or a logger at `level DEBUG`, writes full request URIs, query-string
//! credentials included, into the journal.

mod common;

use common::caddy::{MAIN_CONFIG, main_config};
use common::units::fleet_units;

/// The unit's `RuntimeDirectory=`, which systemd creates under `/run` owned by
/// the unit's user. Caddy creates the socket with its own default mode; what
/// keeps other users out is this directory.
const RUNTIME_DIRECTORY: &str = "caddy";

/// The top-level directives of the leading `{ ... }` block, one trimmed line
/// each. Lines inside nested blocks (`log { ... }`, `servers { ... }`) are not
/// global options and are skipped.
fn global_options(config: &str) -> Vec<String> {
    let mut lines = config
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    assert_eq!(
        lines.next(),
        Some("{"),
        "{MAIN_CONFIG} must open with its global options block:\n{config}"
    );
    let mut depth = 1;
    let mut block = Vec::new();
    for line in lines {
        if line.starts_with('}') {
            depth -= 1;
            if depth == 0 {
                return block;
            }
            continue;
        }
        if depth == 1 {
            block.push(line.to_string());
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    panic!("{MAIN_CONFIG}'s global options block never closes:\n{config}")
}

fn directive<'a>(block: &'a [String], name: &str) -> Vec<&'a str> {
    block
        .iter()
        .filter(|l| l.split_whitespace().next() == Some(name))
        .map(String::as_str)
        .collect()
}

#[test]
fn test_global_options_do_not_enable_debug() {
    let config = main_config();
    assert!(
        directive(&global_options(&config), "debug").is_empty(),
        "{MAIN_CONFIG} turns on `debug`, which logs full request URIs to the journal"
    );
    let debug_level = config
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|words| matches!(words.as_slice(), ["level", level] if level.eq_ignore_ascii_case("debug")));
    assert!(
        debug_level.is_none(),
        "{MAIN_CONFIG} sets a logger to `level DEBUG`, which logs full request URIs to the journal"
    );
}

#[test]
fn test_admin_endpoint_is_a_unix_socket_in_the_private_runtime_dir() {
    let block = global_options(&main_config());
    let admin = directive(&block, "admin");
    let [line] = admin.as_slice() else {
        panic!(
            "{MAIN_CONFIG} must set `admin` exactly once; unset, caddy listens on TCP \
             localhost:2019 with no auth. Found: {admin:?}"
        )
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    let [_, address] = words.as_slice() else {
        panic!("`{line}` must name exactly one address, a unix socket")
    };
    let path = address.strip_prefix("unix/").unwrap_or_else(|| {
        panic!("`{line}` must listen on a unix socket, not TCP; `admin off` breaks reload")
    });
    let dir = format!("/run/{RUNTIME_DIRECTORY}/");
    let file = path.strip_prefix(&dir).unwrap_or("");
    assert!(
        !file.is_empty() && !file.contains('/'),
        "`{line}` must put its socket directly in {dir}, the directory only caddy can enter"
    );
}

/// Read as systemd does: `[Service]` only, the unit file first and each
/// drop-in after it, the last assignment winning.
#[test]
fn test_the_socket_dir_is_the_units_private_runtime_directory() {
    let mut files: Vec<_> = fleet_units()
        .into_iter()
        .filter(|u| u.role == "caddy" && u.name == "caddy.service")
        .collect();
    assert!(
        !files.is_empty(),
        "the caddy role installs no caddy.service"
    );
    files.sort_by_key(|u| u.dropin.clone());
    let effective = |key: &str| files.iter().rev().find_map(|u| u.last_in("Service", key));
    for (key, expected) in [
        ("User", "caddy"),
        ("RuntimeDirectory", RUNTIME_DIRECTORY),
        ("RuntimeDirectoryMode", "0700"),
    ] {
        assert_eq!(
            effective(key),
            Some(expected),
            "caddy.service must set `{key}={expected}` under [Service], or the admin \
             socket is reachable by other users"
        );
    }
}
