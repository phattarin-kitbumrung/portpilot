use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn help_includes_cross_platform_commands() {
    let mut cmd = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("watch"))
        .stdout(predicate::str::contains("dev"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn list_command_executes() {
    let mut cmd = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    cmd.arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("PORT"));
}

#[test]
fn dev_command_executes() {
    let temp = tempdir().expect("temp dir should be created");
    let mut cmd = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    cmd.current_dir(temp.path());
    cmd.arg("dev")
        .assert()
        .success()
        .stdout(predicate::str::contains("PortPilot Dev Services"))
        .stdout(predicate::str::contains("Commands:"));

    let config = fs::read_to_string(temp.path().join("portpilot.toml"))
        .expect("portpilot.toml should be created");
    assert!(config.contains("[dev]"));
    assert!(config.contains("services = []"));
}

#[test]
fn dev_add_and_remove_service_updates_config() {
    let temp = tempdir().expect("temp dir should be created");

    let mut init = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    init.current_dir(temp.path()).arg("dev").assert().success();

    let mut add = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    add.current_dir(temp.path())
        .args(["dev", "add", "rabbitmq"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added service 'rabbitmq'"));

    let config_after_add = fs::read_to_string(temp.path().join("portpilot.toml"))
        .expect("config should exist after add");
    assert!(config_after_add.contains("name = \"rabbitmq\""));
    assert!(!config_after_add.contains("port"));

    let mut remove = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    remove
        .current_dir(temp.path())
        .args(["dev", "remove", "rabbitmq"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed service 'rabbitmq'"));

    let config_after_remove = fs::read_to_string(temp.path().join("portpilot.toml"))
        .expect("config should exist after remove");
    assert!(!config_after_remove.contains("name = \"rabbitmq\""));
}

#[test]
fn dev_add_unknown_service_fails() {
    let temp = tempdir().expect("temp dir should be created");
    let mut add = Command::cargo_bin("portpilot").expect("portpilot binary should build");
    add.current_dir(temp.path())
        .args(["dev", "add", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in the catalog"));
}

#[test]
fn dev_up_and_down_with_empty_config_succeed() {
    let temp = tempdir().expect("temp dir should be created");
    for sub in ["up", "down"] {
        let mut cmd = Command::cargo_bin("portpilot").expect("portpilot binary should build");
        cmd.current_dir(temp.path())
            .args(["dev", sub])
            .assert()
            .success();
    }
}
