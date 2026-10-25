//! sshd hardening wins over provider and cloud-init drop-ins.
//!
//! sshd keeps the first value it reads for a keyword, and `Include` splices
//! the drop-ins in place. A drop-in read before the hardening directives
//! therefore decides them. The rendered config must read every hardening
//! directive before the Include, and the deploy must check what sshd
//! actually resolved.

use std::collections::{BTreeMap, HashMap};
use std::fs;

use serde_yaml::Value;

mod common;

use common::{field, role_dir, role_tasks};

const HOSTILE_DROP_IN: &str = "\
PasswordAuthentication yes
KbdInteractiveAuthentication yes
ChallengeResponseAuthentication yes
PermitEmptyPasswords yes
PermitRootLogin yes
PubkeyAuthentication no
Match all
";

const HARDENED: [(&str, &str); 6] = [
    ("passwordauthentication", "no"),
    ("kbdinteractiveauthentication", "no"),
    ("challengeresponseauthentication", "no"),
    ("permitemptypasswords", "no"),
    ("permitrootlogin", "no"),
    ("pubkeyauthentication", "yes"),
];

fn render_sshd_config() -> String {
    let path = role_dir("ssh").join("templates/sshd_config.j2");
    let template = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let ctx: HashMap<&str, &str> = [
        "ssh_port",
        "ssh_max_auth_tries",
        "ssh_max_sessions",
        "ssh_login_grace_time",
        "ssh_log_level",
        "ssh_client_alive_interval",
        "ssh_client_alive_count_max",
    ]
    .into_iter()
    .map(|key| (key, "1"))
    .collect();
    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("sshd_config.j2 must render")
}

fn effective_global_config(main: &str, drop_in: &str) -> BTreeMap<String, String> {
    let mut effective = BTreeMap::new();
    let lines = main.lines().flat_map(|line| {
        if line.trim_start().to_lowercase().starts_with("include ") {
            drop_in.lines().collect::<Vec<_>>()
        } else {
            vec![line]
        }
    });
    for line in lines {
        let mut words = line.split_whitespace();
        let Some(keyword) = words.next().filter(|w| !w.starts_with('#')) else {
            continue;
        };
        let keyword = keyword.to_lowercase();
        if keyword == "match" {
            break;
        }
        let value = words.next().unwrap_or_default().to_string();
        effective.entry(keyword).or_insert(value);
    }
    effective
}

#[test]
fn test_the_rendered_config_still_includes_the_drop_ins() {
    let rendered = render_sshd_config();
    assert!(
        rendered
            .lines()
            .any(|line| line.trim() == "Include /etc/ssh/sshd_config.d/*.conf"),
        "sshd_config must keep including /etc/ssh/sshd_config.d"
    );
}

#[test]
fn test_hardening_wins_over_a_hostile_drop_in() {
    let effective = effective_global_config(&render_sshd_config(), HOSTILE_DROP_IN);
    for (keyword, expected) in HARDENED {
        assert_eq!(
            effective.get(keyword).map(String::as_str),
            Some(expected),
            "a drop-in must not override `{keyword}`"
        );
    }
}

#[test]
fn test_the_deploy_asserts_the_effective_auth_settings() {
    let tasks = role_tasks("ssh");
    let probe = tasks
        .iter()
        .find(|task| {
            field(&task.body, "ansible.builtin.command")
                .and_then(|module| field(module.as_mapping()?, "cmd"))
                .and_then(Value::as_str)
                .is_some_and(|cmd| cmd.contains("sshd -T"))
        })
        .expect("the ssh role must read the effective config with `sshd -T`");
    let register = field(&probe.body, "register")
        .and_then(Value::as_str)
        .expect("the `sshd -T` probe must register its output");

    let asserted: String = tasks
        .iter()
        .filter_map(|task| field(&task.body, "ansible.builtin.assert"))
        .map(|assert| serde_yaml::to_string(assert).expect("assert serialises"))
        .filter(|assert| assert.contains(register))
        .collect();
    for line in [
        "passwordauthentication no",
        "kbdinteractiveauthentication no",
    ] {
        assert!(
            asserted.contains(line),
            "the ssh role must assert `{register}` reports `{line}`"
        );
    }
}
