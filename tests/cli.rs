use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_workdir(test_name: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("virtualfacility-{test_name}-{stamp}"));
    fs::create_dir_all(&dir).expect("temp workdir should be created");
    dir
}

#[test]
fn create_bridge_without_name_only_prints_hint_even_with_saved_context() {
    let workdir = temp_workdir("create-bridge-hint");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let use_output = Command::new(bin)
        .current_dir(&workdir)
        .args(["use", "lab1"])
        .output()
        .expect("use command should run");
    assert!(
        use_output.status.success(),
        "use command failed: {}",
        String::from_utf8_lossy(&use_output.stderr)
    );

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "bridge"])
        .output()
        .expect("create bridge command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bridge name is required."));
    assert!(stderr.contains("suggested: vf-lab1"));
    assert!(stderr.contains("example: cargo run -- create bridge vf-lab1"));
    assert!(!stderr.contains("creating bridge"));
}

#[test]
fn delete_bridge_rejects_extra_targets() {
    let workdir = temp_workdir("delete-bridge-extra");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "bridge", "client", "server", "proxy"])
        .output()
        .expect("delete bridge command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("delete bridge accepts one target only"));
    assert!(stderr.contains("unexpected extra arguments: server proxy"));
    assert!(!stderr.contains("deleting bridge"));
}

#[test]
fn delete_pod_rejects_extra_targets() {
    let workdir = temp_workdir("delete-pod-extra");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "pod", "client", "server", "proxy"])
        .output()
        .expect("delete pod command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("delete pod accepts one target only"));
    assert!(stderr.contains("unexpected extra arguments: server proxy"));
    assert!(!stderr.contains("deleting pod"));
}

#[test]
fn delete_node_requires_explicit_target() {
    let workdir = temp_workdir("delete-node-missing");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "node"])
        .output()
        .expect("delete node command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("delete node requires a target name"));
    assert!(!stderr.contains("deleting node"));
}

#[test]
fn delete_unknown_pod_reports_unknown_before_running_delete() {
    let workdir = temp_workdir("delete-unknown-pod");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "pod", "ghost"])
        .output()
        .expect("delete pod command should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown pod `ghost`"));
    assert!(!stderr.contains("deleting pod"));
}
