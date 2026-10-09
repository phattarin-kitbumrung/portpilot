use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::net::TcpListener;
use tempfile::tempdir;

fn portpilot() -> Command {
    Command::cargo_bin("portpilot").expect("portpilot binary should build")
}

fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

#[test]
fn list_shows_listening_port() {
    let (_l, port) = listener();
    portpilot()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(port.to_string()))
        .stdout(predicate::str::contains("LISTEN"));
}

#[test]
fn bare_invocation_defaults_to_list() {
    portpilot()
        .assert()
        .success()
        .stdout(predicate::str::contains("PROTOCOL"));
}

#[test]
fn list_search_filters_rows() {
    let (_l, port) = listener();
    portpilot()
        .args(["list", "--search", &port.to_string()])
        .assert()
        .success()
        .stdout(predicate::str::contains(port.to_string()));

    portpilot()
        .args(["list", "--search", "definitely-no-such-process-xyz"])
        .assert()
        .success()
        .stdout(predicate::str::contains("LISTEN").not());
}

#[test]
fn port_command_reports_details() {
    let (_l, port) = listener();
    portpilot()
        .args(["port", &port.to_string()])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("Port:       {port}")))
        .stdout(predicate::str::contains("Protocol:   TCP"))
        .stdout(predicate::str::contains("Status:     LISTEN"));
}

#[test]
fn port_command_unknown_port_fails() {
    portpilot()
        .args(["port", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("port 1 not found"));
}

#[test]
fn port_command_rejects_non_numeric_and_out_of_range() {
    for bad in ["abc", "70000", "-1"] {
        portpilot().args(["port", bad]).assert().failure();
    }
}

#[test]
fn process_command_finds_by_pid_of_listener_owner() {
    let (_l, port) = listener();
    let output = portpilot()
        .args(["port", &port.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let pid = text
        .lines()
        .find_map(|l| l.strip_prefix("PID:"))
        .expect("PID line")
        .trim()
        .to_string();

    portpilot()
        .args(["process", &pid])
        .assert()
        .success()
        .stdout(predicate::str::contains(port.to_string()));
}

#[test]
fn process_command_unknown_and_empty_fail() {
    portpilot()
        .args(["process", "definitely-no-such-process-xyz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
    portpilot()
        .args(["process", "  "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be empty"));
}

#[test]
fn inspect_command_reports_listener_owner() {
    let (_l, port) = listener();
    let out = portpilot()
        .args(["port", &port.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let pid = text
        .lines()
        .find_map(|l| l.strip_prefix("PID:"))
        .unwrap()
        .trim()
        .to_string();

    portpilot()
        .args(["inspect", &pid])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("PID:       {pid}")))
        .stdout(predicate::str::contains(port.to_string()));
}

#[test]
fn inspect_unknown_pid_fails() {
    portpilot()
        .args(["inspect", "4294967000"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn kill_rejects_non_numeric_target() {
    portpilot().args(["kill", "abc"]).assert().failure();
}

#[test]
fn kill_unknown_pid_fails_without_prompt() {
    portpilot()
        .args(["kill", "4294967000"])
        .write_stdin("y\n")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Kill this process?").not());
}

#[test]
fn kill_declined_leaves_process_running() {
    let (_l, port) = listener();
    for answer in ["n\n", "\n", "yes please\n"] {
        portpilot()
            .args(["kill", &port.to_string()])
            .write_stdin(answer)
            .assert()
            .success()
            .stdout(predicate::str::contains("Kill this process? [y/N]"))
            .stdout(predicate::str::contains("Aborted"));
    }
    // Still bound: the listener (this test process) survived.
    portpilot()
        .args(["port", &port.to_string()])
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn kill_confirmed_terminates_process() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    portpilot()
        .args(["kill", &pid.to_string()])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Process {pid} terminated"
        )));

    let status = child.wait().expect("wait");
    assert!(!status.success());
}

#[test]
fn dev_add_is_case_insensitive_and_rejects_duplicates() {
    let temp = tempdir().unwrap();
    portpilot()
        .current_dir(temp.path())
        .args(["dev", "add", "Redis"])
        .assert()
        .success();
    portpilot()
        .current_dir(temp.path())
        .args(["dev", "add", "redis"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    let config = fs::read_to_string(temp.path().join("portpilot.toml")).unwrap();
    assert_eq!(config.matches("name = \"redis\"").count(), 1);
}

#[test]
fn dev_remove_missing_service_fails() {
    let temp = tempdir().unwrap();
    portpilot()
        .current_dir(temp.path())
        .args(["dev", "remove", "redis"])
        .assert()
        .failure();
}

#[test]
fn dev_status_lists_configured_services() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("portpilot.toml"),
        "[[dev.services]]\nname = \"redis\"\n",
    )
    .unwrap();
    portpilot()
        .current_dir(temp.path())
        .arg("dev")
        .assert()
        .success()
        .stdout(predicate::str::contains("redis"));
}

#[test]
fn dev_with_invalid_config_reports_parse_error() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("portpilot.toml"), "not = [valid").unwrap();
    portpilot()
        .current_dir(temp.path())
        .arg("dev")
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse"));
}

#[test]
fn local_catalog_overrides_builtin() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("catalog.toml"),
        "[[services]]\nname = \"custom\"\nimage = \"busybox\"\ndefault_port = 8080\n",
    )
    .unwrap();

    portpilot()
        .current_dir(temp.path())
        .args(["dev", "add", "custom"])
        .assert()
        .success();
    portpilot()
        .current_dir(temp.path())
        .args(["dev", "add", "redis"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in the catalog"));
}

#[test]
fn invalid_local_catalog_reports_parse_error() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("catalog.toml"), "garbage = [").unwrap();
    portpilot()
        .current_dir(temp.path())
        .args(["dev", "add", "redis"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse catalog.toml"));
}

#[test]
fn unknown_subcommand_fails_with_usage() {
    portpilot()
        .arg("bogus")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn kill_port_and_pid_flags_conflict() {
    portpilot()
        .args(["kill", "1234", "--port", "--pid"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn kill_port_flag_targets_port_only() {
    portpilot()
        .args(["kill", "1", "--port"])
        .write_stdin("y\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("port 1 not found"));
}

#[test]
fn kill_pid_flag_never_treats_target_as_port() {
    // A listening port number is not a PID, so --pid must not resolve it as a port.
    let (_l, port) = listener();
    portpilot()
        .args(["kill", &port.to_string(), "--pid"])
        .write_stdin("y\n")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Kill this process?").not());
}

#[test]
fn kill_port_flag_resolves_listener() {
    let (_l, port) = listener();
    portpilot()
        .args(["kill", &port.to_string(), "--port"])
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Aborted"));
}
