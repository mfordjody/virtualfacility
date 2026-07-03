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
fn create_bridge_rejects_command_word_name() {
    let workdir = temp_workdir("create-bridge-command-word");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "bridge", "status"])
        .output()
        .expect("create bridge command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid bridge name `status`"));
    assert!(stderr.contains("command words cannot be used as resource names"));
    assert!(!stderr.contains("creating bridge"));
}

#[test]
fn create_bridge_requires_vf_prefix() {
    let workdir = temp_workdir("create-bridge-prefix");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "bridge", "lab1"])
        .output()
        .expect("create bridge command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid bridge name `lab1`"));
    assert!(stderr.contains("bridge names must start with `vf-`"));
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
fn create_custom_node_is_not_rejected_by_static_topology() {
    let workdir = temp_workdir("create-custom-node");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "node", "node-1"])
        .output()
        .expect("create node command should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unknown node `node-1`"));
}

#[test]
fn delete_custom_pod_is_not_rejected_by_static_topology() {
    let workdir = temp_workdir("delete-custom-pod");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "pod", "ghost"])
        .output()
        .expect("delete pod command should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unknown pod `ghost`"));
    assert!(!stderr.contains("deleting pod"));
}

#[test]
fn create_custom_pod_is_not_rejected_by_static_topology() {
    let workdir = temp_workdir("create-custom-pod");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "pod", "app-1"])
        .output()
        .expect("create pod command should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unknown pod `app-1`"));
}

#[test]
fn create_custom_pod_accepts_explicit_node() {
    let workdir = temp_workdir("create-custom-pod-node");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "pod", "app-1", "--node", "node-1"])
        .output()
        .expect("create pod command should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unknown node `node-1`"));
    assert!(!stderr.contains("unknown pod `app-1`"));
}

#[test]
fn bridge_name_selects_distinct_underlay_cidr() {
    let workdir = temp_workdir("bridge-cidr");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let lab1 = Command::new(bin)
        .current_dir(&workdir)
        .args(["--name", "lab1", "--bridge", "vf-lab1", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        lab1.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&lab1.stderr)
    );
    let lab1_stdout = String::from_utf8_lossy(&lab1.stdout);
    assert!(lab1_stdout.contains("bridge: vf-lab1 at 10.200.1.1/24"));
    assert!(lab1_stdout.contains("assign node uplink address `10.200.1.10/24`"));

    let lab2 = Command::new(bin)
        .current_dir(&workdir)
        .args(["--name", "lab2", "--bridge", "vf-lab2", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        lab2.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&lab2.stderr)
    );
    let lab2_stdout = String::from_utf8_lossy(&lab2.stdout);
    assert!(lab2_stdout.contains("bridge: vf-lab2 at 10.200.2.1/24"));
    assert!(lab2_stdout.contains("assign node uplink address `10.200.2.10/24`"));

    let default = Command::new(bin)
        .current_dir(&workdir)
        .args(["--name", "br0", "--bridge", "vf-br0", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        default.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&default.stderr)
    );
    let default_stdout = String::from_utf8_lossy(&default.stdout);
    assert!(default_stdout.contains("bridge: vf-br0 at 10.200.0.1/24"));
    assert!(default_stdout.contains("assign node uplink address `10.200.0.10/24`"));
}
