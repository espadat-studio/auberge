use crate::services::ssh::SshSession;
use eyre::{Result, WrapErr};
use regex::Regex;
use reqwest::{Client, Method};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FOLDER_ID: &str = "agent-memory";
pub const FOLDER_LABEL: &str = "Agent memory";
pub const SYNC_PORT: u16 = 22000;
pub const HOST_SYNCTHING_USER: &str = "syncthing";
const HOST_API: &str = "http://127.0.0.1:8384/rest";
const DEFAULT_LOCAL_API: &str = "127.0.0.1:8384";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub fn canonical_id(id: &str) -> String {
    id.replace('-', "").to_uppercase()
}

fn validate_device_id(id: &str) -> Result<()> {
    let shape = Regex::new(r"^[A-Z2-7]{7}(-[A-Z2-7]{7}){7}$").expect("static pattern");
    eyre::ensure!(shape.is_match(id), "'{id}' is not a Syncthing device ID");
    Ok(())
}

pub fn host_config_path() -> String {
    format!("/home/{HOST_SYNCTHING_USER}/.local/state/syncthing/config.xml")
}

pub fn device_id_command(become_method: &str) -> String {
    let config = host_config_path();
    format!(
        "key=$({become_method} -n sed -n 's:.*<apikey>\\(.*\\)</apikey>.*:\\1:p' {config}) \
         || {{ echo 'cannot read {config}' >&2; exit 1; }}; \
         [ -n \"$key\" ] || {{ echo 'no apikey in {config}' >&2; exit 1; }}; \
         curl -fsS --max-time 10 -H \"X-API-Key: $key\" {HOST_API}/system/status"
    )
}

pub fn read_device_id(session: &dyn SshSession, host_name: &str) -> Result<String> {
    let result = session
        .run(&device_id_command(session.become_method()))
        .wrap_err_with(|| {
            format!("could not reach '{host_name}' to read its Syncthing device ID")
        })?;
    if !result.success {
        let stderr = result.stderr_str();
        eyre::bail!(
            "reading the Syncthing device ID on '{host_name}' failed: {}",
            stderr.trim()
        );
    }
    parse_device_id(&result.stdout_str())
        .wrap_err_with(|| format!("'{host_name}' answered /rest/system/status unexpectedly"))
}

pub fn parse_device_id(status_json: &str) -> Result<String> {
    let status: Value = serde_json::from_str(status_json).wrap_err("response is not JSON")?;
    let id = status["myID"]
        .as_str()
        .ok_or_else(|| eyre::eyre!("response has no myID"))?;
    validate_device_id(id)?;
    Ok(id.to_string())
}

pub fn default_local_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("could not determine home directory"))?;
    let state = home.join(".local/state/syncthing/config.xml");
    if state.exists() {
        return Ok(state);
    }
    Ok(home.join(".config/syncthing/config.xml"))
}

pub fn default_folder_path(host_name: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("could not determine home directory"))?;
    Ok(home.join("memories").join(host_name))
}

pub struct LocalCredentials {
    pub api_key: String,
    pub address: String,
}

pub fn parse_local_credentials(config_xml: &str) -> Result<LocalCredentials> {
    let key = Regex::new(r"<apikey>([^<]+)</apikey>").expect("static pattern");
    let api_key = key
        .captures(config_xml)
        .map(|c| c[1].to_string())
        .ok_or_else(|| eyre::eyre!("no <apikey> in the local Syncthing config"))?;
    let gui = Regex::new(r"(?s)<gui[^>]*>.*?<address>([^<]+)</address>").expect("static pattern");
    let address = gui
        .captures(config_xml)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| DEFAULT_LOCAL_API.to_string());
    Ok(LocalCredentials { api_key, address })
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub stale_devices: Vec<String>,
    pub device: Option<Value>,
    pub folder: Option<Value>,
}

impl Plan {
    pub fn is_converged(&self) -> bool {
        self.stale_devices.is_empty() && self.device.is_none() && self.folder.is_none()
    }
}

pub struct Desired<'a> {
    pub host_name: &'a str,
    pub host_id: &'a str,
    pub host_address: &'a str,
    pub folder_path: &'a Path,
}

fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return PathBuf::from(path);
    };
    dirs::home_dir().unwrap_or_default().join(rest)
}

fn device_key(device: &Value) -> String {
    canonical_id(device["deviceID"].as_str().unwrap_or_default())
}

pub fn plan_pairing(config: &Value, local_id: &str, desired: &Desired) -> Result<Plan> {
    let host_key = canonical_id(desired.host_id);
    let local_key = canonical_id(local_id);
    eyre::ensure!(
        host_key != local_key,
        "'{}' has this machine's own device ID; pairing a device with itself does nothing",
        desired.host_name
    );
    let devices = config["devices"].as_array().cloned().unwrap_or_default();
    let folders = config["folders"].as_array().cloned().unwrap_or_default();

    let stale_devices: Vec<String> = devices
        .iter()
        .filter(|d| d["name"] == desired.host_name && device_key(d) != host_key)
        .map(|d| d["deviceID"].as_str().unwrap_or_default().to_string())
        .collect();

    for folder in folders.iter().filter(|f| f["id"] != FOLDER_ID) {
        let other = expand_home(folder["path"].as_str().unwrap_or_default());
        eyre::ensure!(
            !other.starts_with(desired.folder_path) && !desired.folder_path.starts_with(&other),
            "{} is nested in or contains the Syncthing folder '{}' ({}); sharing it would leak the memories to that folder's devices",
            desired.folder_path.display(),
            folder["id"].as_str().unwrap_or_default(),
            other.display()
        );
    }

    let current_device = devices.iter().find(|d| device_key(d) == host_key);
    let device_is_current = current_device.is_some_and(|d| {
        d["name"] == desired.host_name && d["addresses"] == json!([desired.host_address])
    });
    let device = (!device_is_current).then(|| {
        let mut body = current_device
            .cloned()
            .unwrap_or_else(|| json!({ "deviceID": desired.host_id }));
        body["name"] = json!(desired.host_name);
        body["addresses"] = json!([desired.host_address]);
        body
    });

    let current_folder = folders.iter().find(|f| f["id"] == FOLDER_ID);
    if let Some(folder) = current_folder {
        let path = expand_home(folder["path"].as_str().unwrap_or_default());
        eyre::ensure!(
            path == desired.folder_path,
            "folder '{FOLDER_ID}' already exists at {}, and a folder ID is unique per machine; pass --path {} or remove it first",
            path.display(),
            path.display()
        );
        for device in folder["devices"].as_array().into_iter().flatten() {
            let key = device_key(device);
            let is_stale = stale_devices.iter().any(|s| canonical_id(s) == key);
            eyre::ensure!(
                key == local_key || key == host_key || is_stale,
                "folder '{FOLDER_ID}' is shared with another device ({}); unshare it first",
                device["deviceID"].as_str().unwrap_or_default()
            );
        }
    }

    let desired_devices = json!([{ "deviceID": local_id }, { "deviceID": desired.host_id }]);
    let folder_is_current = current_folder.is_some_and(|f| {
        let mut keys: Vec<String> = f["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(device_key)
            .collect();
        keys.sort();
        let mut want = vec![local_key.clone(), host_key.clone()];
        want.sort();
        f["type"] == "sendreceive"
            && f["paused"] == false
            && f["fsWatcherEnabled"] == true
            && f["fsWatcherDelayS"] == 10
            && f["rescanIntervalS"] == 60
            && keys == want
    });
    let folder = (!folder_is_current).then(|| {
        let mut body = current_folder.cloned().unwrap_or_else(|| {
            json!({
                "id": FOLDER_ID,
                "label": FOLDER_LABEL,
                "path": desired.folder_path.to_string_lossy(),
            })
        });
        body["type"] = json!("sendreceive");
        body["paused"] = json!(false);
        body["fsWatcherEnabled"] = json!(true);
        body["fsWatcherDelayS"] = json!(10);
        body["rescanIntervalS"] = json!(60);
        body["devices"] = desired_devices;
        body
    });

    Ok(Plan {
        stale_devices,
        device,
        folder,
    })
}

pub struct LocalSyncthing {
    client: Client,
    base: String,
    api_key: String,
}

impl LocalSyncthing {
    pub fn new(credentials: LocalCredentials) -> Result<Self> {
        Ok(Self {
            client: Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            base: format!("http://{}/rest", credentials.address),
            api_key: credentials.api_key,
        })
    }

    async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base))
            .header("X-API-Key", &self.api_key);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .wrap_err_with(|| format!("{method} {path} on the local Syncthing failed"))?;
        let status = response.status();
        let text = response.text().await?;
        eyre::ensure!(
            status.is_success(),
            "{method} {path} on the local Syncthing returned {status}: {}",
            text.trim()
        );
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn config(&self) -> Result<Value> {
        self.call(Method::GET, "/config", None).await
    }

    pub async fn my_id(&self) -> Result<String> {
        let status = self.call(Method::GET, "/system/status", None).await?;
        status["myID"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| eyre::eyre!("local /rest/system/status has no myID"))
    }

    pub async fn apply(&self, plan: &Plan) -> Result<()> {
        if let Some(device) = &plan.device {
            let id = device["deviceID"].as_str().unwrap_or_default();
            self.call(Method::PUT, &format!("/config/devices/{id}"), Some(device))
                .await
                .wrap_err("adding the Host as a device failed")?;
        }
        if let Some(folder) = &plan.folder {
            self.call(
                Method::PUT,
                &format!("/config/folders/{FOLDER_ID}"),
                Some(folder),
            )
            .await
            .wrap_err("sharing the folder failed")?;
        }
        for id in &plan.stale_devices {
            self.call(Method::DELETE, &format!("/config/devices/{id}"), None)
                .await
                .wrap_err("removing the stale device failed")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: &str = "AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH";
    const HOST: &str = "BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH-AAAAAAA";
    const OLD_HOST: &str = "CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH-AAAAAAA-BBBBBBB";
    const ADDRESS: &str = "tcp://100.64.0.4:22000";

    fn desired<'a>(path: &'a Path) -> Desired<'a> {
        Desired {
            host_name: "ruche",
            host_id: HOST,
            host_address: ADDRESS,
            folder_path: path,
        }
    }

    fn converged_config() -> Value {
        json!({
            "devices": [
                { "deviceID": LOCAL, "name": "laptop" },
                { "deviceID": HOST, "name": "ruche", "addresses": [ADDRESS] }
            ],
            "folders": [{
                "id": FOLDER_ID, "path": "/home/me/memories/ruche",
                "type": "sendreceive", "paused": false, "fsWatcherEnabled": true,
                "fsWatcherDelayS": 10, "rescanIntervalS": 60,
                "devices": [{ "deviceID": LOCAL }, { "deviceID": HOST }]
            }]
        })
    }

    #[test]
    fn a_peer_that_knows_nothing_gets_the_device_and_the_folder() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let config = json!({ "devices": [{ "deviceID": LOCAL }], "folders": [] });
        let plan = plan_pairing(&config, LOCAL, &desired(&path)).unwrap();
        assert!(plan.stale_devices.is_empty());
        assert_eq!(plan.device.unwrap()["addresses"], json!([ADDRESS]));
        let folder = plan.folder.unwrap();
        assert_eq!(folder["id"], FOLDER_ID);
        assert_eq!(folder["type"], "sendreceive");
        assert_eq!(
            folder["devices"],
            json!([{ "deviceID": LOCAL }, { "deviceID": HOST }])
        );
    }

    #[test]
    fn a_converged_peer_plans_nothing() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let plan = plan_pairing(&converged_config(), LOCAL, &desired(&path)).unwrap();
        assert!(plan.is_converged(), "{plan:?}");
    }

    #[test]
    fn an_id_differing_only_by_dashes_and_case_is_converged() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let mut config = converged_config();
        config["devices"][1]["deviceID"] = json!(HOST.replace('-', "").to_lowercase());
        config["folders"][0]["devices"][1]["deviceID"] = json!(HOST.to_lowercase());
        let plan = plan_pairing(&config, LOCAL, &desired(&path)).unwrap();
        assert!(plan.is_converged(), "{plan:?}");
    }

    #[test]
    fn a_rebuilt_host_replaces_the_stale_entry_of_the_same_name() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let mut config = converged_config();
        config["devices"][1] =
            json!({ "deviceID": OLD_HOST, "name": "ruche", "addresses": [ADDRESS] });
        config["folders"][0]["devices"][1] = json!({ "deviceID": OLD_HOST });
        let plan = plan_pairing(&config, LOCAL, &desired(&path)).unwrap();
        assert_eq!(plan.stale_devices, vec![OLD_HOST.to_string()]);
        assert_eq!(plan.device.unwrap()["deviceID"], HOST);
        assert_eq!(
            plan.folder.unwrap()["devices"],
            json!([{ "deviceID": LOCAL }, { "deviceID": HOST }])
        );
    }

    #[test]
    fn a_changed_address_updates_the_device_and_keeps_its_other_fields() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let mut config = converged_config();
        config["devices"][1]["addresses"] = json!(["dynamic"]);
        config["devices"][1]["compression"] = json!("metadata");
        let plan = plan_pairing(&config, LOCAL, &desired(&path)).unwrap();
        let device = plan.device.unwrap();
        assert_eq!(device["addresses"], json!([ADDRESS]));
        assert_eq!(device["compression"], "metadata");
        assert!(plan.folder.is_none());
    }

    #[test]
    fn a_paused_folder_is_resumed() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let mut config = converged_config();
        config["folders"][0]["paused"] = json!(true);
        let plan = plan_pairing(&config, LOCAL, &desired(&path)).unwrap();
        assert_eq!(plan.folder.unwrap()["paused"], false);
    }

    #[test]
    fn a_path_inside_an_existing_folder_is_refused() {
        let path = PathBuf::from("/home/me/Sync/memories");
        let config =
            json!({ "devices": [], "folders": [{ "id": "default", "path": "/home/me/Sync" }] });
        let err = plan_pairing(&config, LOCAL, &desired(&path)).unwrap_err();
        assert!(err.to_string().contains("nested in or contains"), "{err}");
    }

    #[test]
    fn a_path_above_an_existing_folder_is_refused() {
        let path = PathBuf::from("/home/me/memories");
        let config = json!({ "devices": [], "folders": [{ "id": "notes", "path": "/home/me/memories/notes" }] });
        assert!(plan_pairing(&config, LOCAL, &desired(&path)).is_err());
    }

    #[test]
    fn a_sibling_path_with_a_shared_prefix_is_not_nested() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let config = json!({ "devices": [], "folders": [{ "id": "x", "path": "/home/me/memories/ruche-old" }] });
        assert!(plan_pairing(&config, LOCAL, &desired(&path)).is_ok());
    }

    #[test]
    fn an_existing_folder_at_another_path_is_refused() {
        let path = PathBuf::from("/home/me/elsewhere");
        let err = plan_pairing(&converged_config(), LOCAL, &desired(&path)).unwrap_err();
        assert!(err.to_string().contains("already exists at"), "{err}");
    }

    #[test]
    fn a_folder_shared_with_a_third_device_is_refused() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let mut config = converged_config();
        config["folders"][0]["devices"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "deviceID": "ZZZZZZZ-ZZZZZZZ-ZZZZZZZ-ZZZZZZZ-ZZZZZZZ-ZZZZZZZ-ZZZZZZZ-ZZZZZZZ" }));
        let err = plan_pairing(&config, LOCAL, &desired(&path)).unwrap_err();
        assert!(err.to_string().contains("another device"), "{err}");
    }

    #[test]
    fn pairing_a_host_with_this_machines_own_id_is_refused() {
        let path = PathBuf::from("/home/me/memories/ruche");
        let own = Desired {
            host_id: LOCAL,
            ..desired(&path)
        };
        assert!(plan_pairing(&converged_config(), LOCAL, &own).is_err());
    }

    #[test]
    fn parse_device_id_reads_my_id() {
        let json = format!(r#"{{"myID":"{HOST}","uptime":3}}"#);
        assert_eq!(parse_device_id(&json).unwrap(), HOST);
    }

    #[test]
    fn parse_device_id_rejects_garbage() {
        assert!(parse_device_id("<html>").is_err());
        assert!(parse_device_id(r#"{"uptime":3}"#).is_err());
        assert!(parse_device_id(r#"{"myID":"nope"}"#).is_err());
    }

    #[test]
    fn local_credentials_read_the_key_and_the_gui_address() {
        let xml = "<configuration><gui enabled=\"true\"><address>127.0.0.1:9999</address><apikey>k3y</apikey></gui></configuration>";
        let creds = parse_local_credentials(xml).unwrap();
        assert_eq!(creds.api_key, "k3y");
        assert_eq!(creds.address, "127.0.0.1:9999");
    }

    #[test]
    fn local_credentials_without_a_key_fail() {
        assert!(parse_local_credentials("<configuration/>").is_err());
    }

    #[test]
    fn read_device_id_escalates_with_the_hosts_become_method_and_never_prompts() {
        let mock = crate::services::ssh::MockSshSession::with_become_method("doas");
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(&format!(
            r#"{{"myID":"{HOST}"}}"#
        )));
        assert_eq!(read_device_id(&mock, "ruche").unwrap(), HOST);
        let calls = format!("{:?}", mock.calls());
        assert!(calls.contains("doas -n sed"), "{calls}");
    }

    #[test]
    fn read_device_id_reports_the_remote_stderr() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"no apikey in /x".to_vec(),
        });
        let err = read_device_id(&mock, "ruche").unwrap_err();
        assert!(err.to_string().contains("no apikey in /x"), "{err}");
    }
}
