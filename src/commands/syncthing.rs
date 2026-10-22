use crate::hosts::{HOST_FLAG, Host, select_or_arg};
use crate::output;
use crate::services::ssh::{LiveSshSession, resolve_ssh_key_path};
use crate::services::syncthing::{
    Desired, LocalSyncthing, Plan, SYNC_PORT, default_folder_path, default_local_config_path,
    parse_local_credentials, plan_pairing, read_device_id,
};
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum SyncthingCommands {
    #[command(
        visible_alias = "id",
        about = "Print a Host's Syncthing device ID",
        long_about = "Print a Host's Syncthing device ID.\n\nReads the ID over SSH from the Host's Syncthing REST API. Stdout carries the ID and nothing else, so it can be captured in a script.",
        after_help = "Examples:\n  auberge syncthing device-id -H ruche\n  ID=$(auberge syncthing device-id -H ruche)"
    )]
    DeviceId {
        #[arg(
            short = 'H',
            long,
            help = "Host running Syncthing (prompts if omitted; off a TTY pass it)"
        )]
        host: Option<String>,
    },
    #[command(
        visible_alias = "p",
        about = "Pair this machine's Syncthing with an agent Host",
        long_about = "Pair this machine's Syncthing with an agent Host.\n\nThe Host cannot dial out (ADR-0055), so the peer initiates. This adds the Host as a device at its tailnet address on port 22000, shares the agent-memory folder with it, and replaces a stale entry of the same name left by a rebuilt Host. A rerun on a converged setup changes nothing.\n\nThe peer is the machine running auberge. Its Syncthing must be running locally.",
        after_help = "Examples:\n  auberge syncthing pair -H ruche\n  auberge syncthing pair -H ruche --dry-run\n  auberge syncthing pair -H ruche --path ~/memories/ruche -y"
    )]
    Pair {
        #[arg(
            short = 'H',
            long,
            help = "Agent Host to pair with (prompts if omitted; off a TTY pass it)"
        )]
        host: Option<String>,
        #[arg(
            long,
            value_name = "DIR",
            help = "Local folder path [default: ~/memories/<host>]"
        )]
        path: Option<PathBuf>,
        #[arg(
            long,
            value_name = "FILE",
            help = "Local Syncthing config.xml holding the API key [default: ~/.local/state/syncthing/config.xml]"
        )]
        config_xml: Option<PathBuf>,
        #[arg(short = 'n', long, help = "Show what would change and write nothing")]
        dry_run: bool,
        #[arg(
            short = 'y',
            long,
            help = "Skip the prompt before replacing a stale device"
        )]
        yes: bool,
    },
}

fn connect_session_and_read_id(host: &Host) -> Result<String> {
    let ssh_key = resolve_ssh_key_path(host, None)?;
    let route = crate::services::route::resolve(host, Some(ssh_key))?;
    let session = LiveSshSession::new(&route, &host.become_method)?;
    read_device_id(&session, &host.name)
}

pub fn run_syncthing_device_id(host_arg: Option<String>) -> Result<()> {
    let host = select_or_arg(host_arg, HOST_FLAG)?;
    println!("{}", connect_session_and_read_id(&host)?);
    Ok(())
}

fn describe(plan: &Plan, host: &Host, address: &str) {
    for id in &plan.stale_devices {
        output::info(&format!("remove stale device {id} (named '{}')", host.name));
    }
    if plan.device.is_some() {
        output::info(&format!("add device '{}' at {address}", host.name));
    }
    if plan.folder.is_some() {
        output::info(&format!("share folder agent-memory with '{}'", host.name));
    }
}

pub async fn run_syncthing_pair(
    host_arg: Option<String>,
    path: Option<PathBuf>,
    config_xml: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let host = select_or_arg(host_arg, HOST_FLAG)?;
    let tailscale_ip = host.tailscale_ip.clone().ok_or_else(|| {
        eyre::eyre!(
            "no tailscale_ip cached for '{0}', and the peer must dial its tailnet address. Run `auberge host detect-tailscale-ip {0}`",
            host.name
        )
    })?;
    let address = format!("tcp://{tailscale_ip}:{SYNC_PORT}");
    let folder_path = match path {
        Some(path) => path,
        None => default_folder_path(&host.name)?,
    };

    output::info(&format!("reading the device ID of '{}'", host.name));
    let host_id = connect_session_and_read_id(&host)?;

    let config_path = match config_xml {
        Some(path) => path,
        None => default_local_config_path()?,
    };
    let xml = std::fs::read_to_string(&config_path).wrap_err_with(|| {
        format!(
            "reading the local Syncthing config {} failed (pass --config-xml)",
            config_path.display()
        )
    })?;
    let local = LocalSyncthing::new(parse_local_credentials(&xml)?)?;
    let local_id = local.my_id().await?;
    let config = local.config().await?;

    let plan = plan_pairing(
        &config,
        &local_id,
        &Desired {
            host_name: &host.name,
            host_id: &host_id,
            host_address: &address,
            folder_path: &folder_path,
        },
    )?;

    if plan.is_converged() {
        output::success(&format!(
            "already paired with '{}', nothing to change",
            host.name
        ));
        return Ok(());
    }
    describe(&plan, &host, &address);
    if dry_run {
        output::info("dry run, nothing written");
        return Ok(());
    }
    if !plan.stale_devices.is_empty() && !crate::prompt::confirm("Remove the stale device?", yes) {
        eyre::bail!("cancelled; pass -y to replace the stale device without a prompt");
    }

    if plan.folder.is_some() {
        std::fs::create_dir_all(&folder_path)
            .wrap_err_with(|| format!("creating {} failed", folder_path.display()))?;
    }
    local.apply(&plan).await?;
    output::success(&format!("paired with '{}'", host.name));
    Ok(())
}
