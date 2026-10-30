//! Syncthing's management plane answers only to its credentials.
//!
//! With no GUI user, Syncthing hands a CSRF cookie to any client that loads
//! `/` and accepts it in place of the API key, so every local UID that can open
//! TCP to the loopback reads the API key and rewrites folders, devices and
//! versioning as the `syncthing` identity. Two declarations close that, and
//! neither holds by construction:
//!
//! - the role declares a GUI user and password on every Host it runs on. The
//!   write is gated only on a login with those credentials failing, never on an
//!   opt-in knob, so no Host keeps the unauthenticated default;
//! - the Standby Runner, whose jobs are org-repo code on the agent Host, cannot
//!   reach the loopback at all, except the resolver stub every lookup goes
//!   through. Egress stays open: the jobs need GitHub.

use std::collections::BTreeSet;
use std::fs;

use regex::Regex;
use serde_yaml::Value;

mod common;

use common::units::directives;
use common::{field, parse_yaml, playbooks_dir, role_dir, role_tasks};

const PASSWORD_KEY: &str = "syncthing_gui_password";
const LOGIN_PROBE: &str = "syncthing_gui_auth_is_current";
const RESOLVER_STUB: &str = "127.0.0.53";
const JOB_LOOPBACK: &str = "127.77.0.1";

fn directive_values(unit: &str, key: &str) -> BTreeSet<String> {
    directives(unit)
        .into_iter()
        .filter(|d| d.section == "Service" && d.key == key)
        .flat_map(|d| {
            d.value
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn test_the_syncthing_role_declares_gui_credentials_on_every_host() {
    let writes: Vec<_> = role_tasks("syncthing")
        .into_iter()
        .filter(|task| {
            let Some(uri) = field(&task.body, "ansible.builtin.uri").and_then(Value::as_mapping)
            else {
                return false;
            };
            let url = field(uri, "url")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let method = field(uri, "method").and_then(Value::as_str);
            let body = field(uri, "body").and_then(Value::as_mapping);
            url.ends_with("/config/gui")
                && method == Some("PATCH")
                && body.is_some_and(|body| {
                    field(body, "user").is_some()
                        && field(body, "password")
                            .and_then(Value::as_str)
                            .is_some_and(|password| password.contains(PASSWORD_KEY))
                })
        })
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "the syncthing role must PATCH /config/gui with a user and `{PASSWORD_KEY}` exactly once"
    );

    let knob = Regex::new(r"\bsyncthing_\w+").expect("static pattern");
    let stray: Vec<_> = writes[0]
        .guards
        .iter()
        .flat_map(|guard| knob.find_iter(guard).map(|m| m.as_str().to_string()))
        .filter(|name| name != LOGIN_PROBE)
        .collect();
    assert!(
        stray.is_empty(),
        "the GUI credential write must be gated only on `{LOGIN_PROBE}`, so every Host gets it; found {stray:?}"
    );
}

#[test]
fn test_the_gui_password_is_a_required_secret() {
    let meta = parse_yaml(&playbooks_dir().join("syncthing.meta.yml"));
    let required: Vec<_> = meta
        .get("required_keys")
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        required.contains(&PASSWORD_KEY),
        "syncthing.meta.yml must require `{PASSWORD_KEY}`"
    );

    let registry = parse_yaml(&playbooks_dir().join("../keys.yml"));
    let secret = registry
        .get("keys")
        .and_then(|keys| keys.get(PASSWORD_KEY))
        .and_then(|key| key.get("secret"))
        .and_then(Value::as_bool);
    assert_eq!(
        secret,
        Some(true),
        "`{PASSWORD_KEY}` must be a secret in the Key Registry"
    );
}

#[test]
fn test_the_standby_runner_reaches_no_loopback_service() {
    let path = role_dir("standby").join("templates/standby-runner.service.j2");
    let unit = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    assert_eq!(
        directive_values(&unit, "IPAddressDeny"),
        BTreeSet::from(["localhost".to_string()]),
        "the Standby Runner must deny the loopback, and only the loopback, so jobs keep egress"
    );
    assert_eq!(
        directive_values(&unit, "IPAddressAllow"),
        BTreeSet::from([RESOLVER_STUB.to_string(), JOB_LOOPBACK.to_string()]),
        "the Standby Runner may reach only the resolver stub and its own job range on the loopback"
    );
    assert!(
        directives(&unit).iter().any(|d| d.section == "Service"
            && d.key == "ExecStartPre"
            && d.value == format!("+/usr/sbin/ip address replace {JOB_LOOPBACK}/32 dev lo")),
        "the job loopback must be a lo address, or replies to it leave from 127.0.0.1 and are denied"
    );
    for closer in ["PrivateNetwork", "RestrictAddressFamilies"] {
        assert!(
            directive_values(&unit, closer).is_empty(),
            "`{closer}=` would cut the Standby Runner's egress to GitHub"
        );
    }
}
