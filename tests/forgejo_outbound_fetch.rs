use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn render_app_ini() -> String {
    let template = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible/roles/forgejo/templates/app.ini.j2"),
    )
    .expect("app.ini.j2 must exist");

    let mut ctx: HashMap<&str, String> = HashMap::new();
    for key in [
        "forgejo_app_name",
        "forgejo_sys_user",
        "forgejo_data_dir",
        "forgejo_port",
        "forgejo_domain",
        "forgejo_config_dir",
        "forgejo_tailscale_ip",
        "forgejo_ssh_domain",
        "forgejo_ssh_port",
    ] {
        ctx.insert(key, "x".to_string());
    }
    ctx.insert("forgejo_cors_allow_origins", String::new());

    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("app.ini.j2 must render")
}

/// The value of `key` under `[section]`, as Forgejo's ini loader would read it.
/// A key under the wrong section is not set at all, so a substring match on
/// the whole file would pass a broken config.
fn ini_value(rendered: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in rendered.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
            continue;
        }
        if current != section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[test]
fn test_webhooks_reach_only_external_hosts() {
    assert_eq!(
        ini_value(&render_app_ini(), "webhook", "ALLOWED_HOST_LIST").as_deref(),
        Some("external")
    );
}

#[test]
fn test_migrations_are_disabled_and_refuse_local_networks() {
    let rendered = render_app_ini();
    assert_eq!(
        ini_value(&rendered, "repository", "DISABLE_MIGRATIONS").as_deref(),
        Some("true")
    );
    assert_eq!(
        ini_value(&rendered, "migrations", "ALLOW_LOCALNETWORKS").as_deref(),
        Some("false")
    );
}

#[test]
fn test_non_admin_accounts_cannot_create_repositories() {
    assert_eq!(
        ini_value(&render_app_ini(), "repository", "MAX_CREATION_LIMIT").as_deref(),
        Some("0")
    );
}
