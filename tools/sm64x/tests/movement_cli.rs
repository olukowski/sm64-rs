//! End-to-end ROM-free native slice over authored initialization/inputs only.
use serde_json::Value;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const EXAMPLE: &[u8] = include_bytes!("../../../docs/examples/authored-movement.json");

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sm64x"));
    command.env("SM64_ROM", "/does/not/exist/authored-movement-test.z64");
    command
}

#[test]
fn synthetic_example_runs_and_emits_a_valid_independent_native_trace() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/examples/authored-movement.json");
    let before = fs::read(&path).unwrap();
    for json_flag in [false, true] {
        let mut c = command();
        c.args(["movement", "run"]).arg(&path);
        if json_flag {
            c.arg("--json");
        }
        let output = c.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trace = sm64_sim::reference::Trace::read(output.stdout.as_slice()).unwrap();
        assert_eq!(trace.manifest.source, sm64_sim::reference::Source::Native);
        assert_eq!(trace.frames.len(), 20);
        assert_eq!(
            trace.frames.last().unwrap().state.action,
            sm64_sim::movement::BRAKING_STOP
        );
        assert!(trace.manifest.initialization.contains("Invented"));
    }
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        command()
            .args(["movement", "--help"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn malformed_unsupported_or_incomplete_scenarios_fail_without_partial_stdout() {
    let directory = std::env::temp_dir().join(format!(
        "sm64-movement-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("authored.json");
    let original: Value = serde_json::from_slice(EXAMPLE).unwrap();
    let mut cases = vec![b"{".to_vec(), vec![b' '; 1024 * 1024 + 1]];
    for i in 0..7 {
        let mut value = original.clone();
        match i {
            0 => value["segments"][1]["input"]["buttons"] = 32768.into(),
            1 => value["segments"][1]["input"]["stick_y"] = 79.into(),
            2 => value["segments"][2]["ticks"] = 4.into(),
            3 => value["segments"][2]["ticks"] = 6.into(),
            4 => value["lane"]["min_z"] = 1023.into(),
            5 => value["unexpected"] = true.into(),
            _ => value["version"] = 2.into(),
        }
        cases.push(serde_json::to_vec(&value).unwrap());
    }
    for data in cases {
        fs::write(&path, &data).unwrap();
        let output = command()
            .args(["movement", "run"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read(&path).unwrap(), data);
    }
    fs::remove_dir_all(directory).unwrap();
}
