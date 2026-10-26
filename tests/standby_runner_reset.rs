//! The Standby Runner's per-job reset runs inside the unit's mount namespace,
//! where every `ReadWritePaths` entry is a mount point. A mount point cannot be
//! removed, so the reset may empty a granted directory but never delete it
//! (#1203).

use std::collections::BTreeMap;
use std::fs;

mod common;

use common::units::{InstalledUnit, fleet_units};
use common::{defaults, resolve, role_dir};

const UNIT: &str = "standby-runner.service";
const RESET: &str = "files/standby-runner-reset.sh";

/// The paths a line of shell removes outright: every operand of `rm` and
/// `rmdir`, and the start point of a `find -delete` without `-mindepth`, which
/// deletes the start point last.
fn removed_operands(line: &str) -> Vec<String> {
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| word.trim_matches('"').to_string())
        .collect();
    match words.first().map(String::as_str) {
        Some("rm" | "rmdir") => words[1..]
            .iter()
            .filter(|word| !word.starts_with('-'))
            .cloned()
            .collect(),
        Some("find")
            if words.iter().any(|w| w == "-delete") && !words.iter().any(|w| w == "-mindepth") =>
        {
            words.get(1).cloned().into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// A directive's paths, resolved through the role's defaults.
fn paths(unit: &InstalledUnit, key: &str, vars: &BTreeMap<String, String>) -> Vec<String> {
    unit.all_in("Service", key)
        .into_iter()
        .flat_map(|value| {
            resolve(value, vars)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The unit's `ReadWritePaths`, its `ReadOnlyPaths`, and its `Environment`
/// keyed as the reset spells a variable, `${NAME}`.
fn unit_paths() -> (Vec<String>, Vec<String>, BTreeMap<String, String>) {
    let vars = defaults("standby");
    let units = fleet_units();
    let unit = units
        .iter()
        .find(|unit| unit.role == "standby" && unit.name == UNIT && unit.dropin.is_none())
        .unwrap_or_else(|| panic!("the standby role must install {UNIT}"));
    let env = unit
        .all_in("Service", "Environment")
        .into_iter()
        .filter_map(|value| value.split_once('='))
        .map(|(key, value)| (format!("${{{key}}}"), resolve(value, &vars)))
        .collect();
    (
        paths(unit, "ReadWritePaths", &vars),
        paths(unit, "ReadOnlyPaths", &vars),
        env,
    )
}

#[test]
fn test_the_reset_removes_no_path_the_unit_mounts() {
    let (grants, _, env) = unit_paths();

    let path = role_dir("standby").join(RESET);
    let script = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let removed: Vec<String> = script
        .lines()
        .flat_map(removed_operands)
        .filter_map(|operand| env.get(operand.trim_end_matches('/')).cloned())
        .collect();
    assert!(
        !removed.is_empty(),
        "the scan resolved no path the reset removes; it reads nothing and would pass \
         whatever the reset deletes"
    );

    for path in &removed {
        assert!(
            !grants.contains(path),
            "the reset removes `{path}`, which {UNIT} lists under ReadWritePaths; inside the \
             unit's namespace that is a mount point and `rm` fails with EBUSY on every start \
             -- empty it instead"
        );
    }
}

/// The reset hard-links the release into the job dir, and link() fails with
/// EXDEV across mounts, so both must sit inside one `ReadWritePaths` entry. The
/// release stays read-only to the runner at the mount, not only by ownership.
#[test]
fn test_the_release_and_the_job_dir_share_one_mount() {
    let (grants, read_only, env) = unit_paths();
    let release = &env["${STANDBY_RUNNER_RELEASE_DIR}"];
    let job = &env["${STANDBY_RUNNER_JOB_DIR}"];
    let within = |path: &str, grant: &str| path.starts_with(&format!("{grant}/"));

    assert!(
        grants
            .iter()
            .any(|grant| within(release, grant) && within(job, grant)),
        "{UNIT} must grant one directory holding both {release} and {job}; the reset \
         hard-links one into the other, and link() fails with EXDEV across mounts"
    );
    assert!(
        read_only.contains(release),
        "{UNIT} must list {release} under ReadOnlyPaths, so job code cannot write the \
         release even inside the granted install dir (ADR-0085)"
    );
}
