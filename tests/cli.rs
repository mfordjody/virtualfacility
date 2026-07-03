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
fn create_network_without_name_only_prints_hint_even_with_saved_context() {
    let workdir = temp_workdir("create-network-hint");
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
        .args(["create", "network"])
        .output()
        .expect("create network command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("network name is required."));
    assert!(stderr.contains("suggested: vf-lab1"));
    assert!(stderr.contains("example: cargo run -- create network vf-lab1"));
    assert!(!stderr.contains("creating network"));
}

#[test]
fn create_network_rejects_command_word_name() {
    let workdir = temp_workdir("create-network-command-word");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "network", "status"])
        .output()
        .expect("create network command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid network name `status`"));
    assert!(stderr.contains("command words cannot be used as resource names"));
    assert!(!stderr.contains("creating network"));
}

#[test]
fn create_network_requires_vf_prefix() {
    let workdir = temp_workdir("create-network-prefix");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["create", "network", "lab1"])
        .output()
        .expect("create network command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid network name `lab1`"));
    assert!(stderr.contains("network names must start with `vf-`"));
    assert!(!stderr.contains("creating network"));
}

#[test]
fn delete_network_rejects_extra_targets() {
    let workdir = temp_workdir("delete-network-extra");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "network", "client", "server", "proxy"])
        .output()
        .expect("delete network command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("delete network accepts one target only"));
    assert!(stderr.contains("unexpected extra arguments: server proxy"));
    assert!(!stderr.contains("deleting network"));
}

#[test]
fn delete_pod_requires_explicit_target() {
    let workdir = temp_workdir("delete-pod-missing");
    let bin = env!("CARGO_BIN_EXE_virtualfacility");

    let output = Command::new(bin)
        .current_dir(&workdir)
        .args(["delete", "pod"])
        .output()
        .expect("delete pod command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("delete pod requires a target name"));
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
        .args(["--name", "lab1", "--network", "vf-lab1", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        lab1.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&lab1.stderr)
    );
    let lab1_stdout = String::from_utf8_lossy(&lab1.stdout);
    assert!(lab1_stdout.contains("network: vf-lab1 at 10.200.1.1/24"));
    assert!(lab1_stdout.contains("assign node uplink address `10.200.1.10/24`"));

    let lab2 = Command::new(bin)
        .current_dir(&workdir)
        .args(["--name", "lab2", "--network", "vf-lab2", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        lab2.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&lab2.stderr)
    );
    let lab2_stdout = String::from_utf8_lossy(&lab2.stdout);
    assert!(lab2_stdout.contains("network: vf-lab2 at 10.200.2.1/24"));
    assert!(lab2_stdout.contains("assign node uplink address `10.200.2.10/24`"));

    let default = Command::new(bin)
        .current_dir(&workdir)
        .args(["--name", "br0", "--network", "vf-br0", "plan"])
        .output()
        .expect("plan command should run");
    assert!(
        default.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&default.stderr)
    );
    let default_stdout = String::from_utf8_lossy(&default.stdout);
    assert!(default_stdout.contains("network: vf-br0 at 10.200.0.1/24"));
    assert!(default_stdout.contains("assign node uplink address `10.200.0.10/24`"));
}
