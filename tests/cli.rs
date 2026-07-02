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
