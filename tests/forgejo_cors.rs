use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn forgejo_role_dir() -> PathBuf {
    repo_root().join("ansible/roles/forgejo")
}

/// Render app.ini.j2 the way ansible will: ansible.builtin.template defaults
/// trim_blocks to true, which is what keeps the `[cors]` block from leaving a
/// stray blank line or joining onto its neighbour (#582 review's lesson,
/// applied here).
fn render_app_ini(cors_allow_origins: &str) -> String {
    let template = fs::read_to_string(forgejo_role_dir().join("templates/app.ini.j2"))
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
    ctx.insert("forgejo_cors_allow_origins", cors_allow_origins.to_string());

    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("app.ini.j2 must render")
}

#[test]
fn test_cors_is_absent_by_default() {
    let rendered = render_app_ini("");
    assert!(!rendered.contains("[cors]"));
    assert!(!rendered.contains("ENABLED = true"));
}

#[test]
fn test_cors_section_carries_the_configured_origins_verbatim() {
    let origins =
        "https://ma-instalaciones.com,https://*.ma-instalaciones.pages.dev,http://localhost:4321";
    let rendered = render_app_ini(origins);
    assert!(rendered.contains("[cors]"));
    assert!(rendered.contains("ENABLED = true"));
    assert!(rendered.contains(&format!("ALLOW_DOMAIN = {origins}")));
}

#[test]
fn test_cors_headers_allow_the_authorization_header_decap_sends() {
    let rendered = render_app_ini("https://example.com");
    let headers_line = rendered
        .lines()
        .find(|line| line.trim_start().starts_with("HEADERS"))
        .expect("a [cors] block must set HEADERS");
    assert!(headers_line.contains("Authorization"));
}

#[test]
fn test_section_after_cors_survives_ansible_trim_blocks() {
    for origins in ["", "https://example.com"] {
        let rendered = render_app_ini(origins);
        assert!(
            rendered
                .lines()
                .any(|line| line.trim() == "DISABLE_REGISTRATION = true"),
            "[service] must keep its own line (origins={origins:?}):\n{rendered}"
        );
    }
}

#[test]
fn test_role_defaults_declare_cors_origins_empty() {
    let defaults = fs::read_to_string(forgejo_role_dir().join("defaults/main.yml"))
        .expect("forgejo defaults must exist");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&defaults).unwrap();
    assert_eq!(parsed["forgejo_cors_allow_origins"].as_str(), Some(""));
}

#[test]
fn test_key_registry_offers_the_operator_the_cors_key() {
    let keys = fs::read_to_string(repo_root().join("ansible/keys.yml")).expect("keys.yml");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&keys).unwrap();
    let key = &parsed["keys"]["forgejo_cors_allow_origins"];
    assert!(
        key.is_mapping(),
        "forgejo_cors_allow_origins missing from keys.yml"
    );
    assert_eq!(key["secret"].as_bool(), Some(false));
}
