use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn render_app_ini() -> String {
    let template =
        fs::read_to_string(repo_root().join("ansible/roles/forgejo/templates/app.ini.j2"))
            .expect("app.ini.j2 must exist");

    let mut ctx: HashMap<&str, String> = HashMap::new();
    for key in [
        "forgejo_app_name",
        "forgejo_sys_user",
        "forgejo_data_dir",
        "forgejo_port",
        "forgejo_domain",
        "forgejo_config_dir",
    ] {
        ctx.insert(key, "x".to_string());
    }
    ctx.insert("forgejo_cors_allow_origins", String::new());
    ctx.insert("forgejo_tailscale_ip", "100.64.0.7".to_string());
    ctx.insert("forgejo_ssh_domain", "100.64.0.7".to_string());
    ctx.insert("forgejo_ssh_port", "2222".to_string());

    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("app.ini.j2 must render")
}

#[test]
fn test_ssh_server_binds_the_tailnet_address_only() {
    let rendered = render_app_ini();
    assert!(rendered.contains("START_SSH_SERVER = true"));
    assert!(rendered.contains("DISABLE_SSH = false"));
    assert!(rendered.contains("SSH_LISTEN_HOST = 100.64.0.7"));
    assert!(rendered.contains("SSH_LISTEN_PORT = 2222"));
    assert!(rendered.contains("SSH_DOMAIN = 100.64.0.7"));
}

#[test]
fn test_no_authorized_keys_file_is_written() {
    assert!(render_app_ini().contains("SSH_CREATE_AUTHORIZED_KEYS_FILE = false"));
}

#[test]
fn test_ssh_port_is_gated_on_the_forge_role_alone() {
    for rel in [
        "ansible/roles/ufw/tasks/main.yml",
        "ansible/roles/tailscale/tasks/main.yml",
        "ansible/roles/headscale/files/policy.hujson",
    ] {
        let body = fs::read_to_string(repo_root().join(rel)).expect("file must exist");
        assert!(
            !body.contains("forgejo") && !body.contains("2222"),
            "{rel} must carry nothing for the forge: a Host without it changes nothing"
        );
    }
}
