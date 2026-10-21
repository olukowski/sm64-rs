//! End-to-end trace tooling over authored observations; never runs a ROM.
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

const AUTHORED: &[u8] = include_bytes!("../../../docs/examples/authored-trace.json");
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    directory: PathBuf,
    expected: PathBuf,
    actual: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sm64-trace-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let expected = directory.join("expected.json");
        let actual = directory.join("actual.json");
        fs::write(&expected, AUTHORED).unwrap();
        fs::write(&actual, AUTHORED).unwrap();
        Self {
            directory,
            expected,
            actual,
        }
    }
    fn mutate(&self, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(AUTHORED).unwrap();
        change(&mut value);
        fs::write(&self.actual, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn compare(&self) -> Command {
        let mut command = command();
        command
            .args(["trace", "compare"])
            .arg(&self.expected)
            .arg(&self.actual)
            .arg("--json");
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sm64x"));
    // Trace commands must not even attempt to open a selected ROM.
    command.env("SM64_ROM", "/does/not/exist/sm64-trace-test.n64");
    command
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn documentation_example_validates_and_compares_without_rom() {
    let f = Fixture::new();
    let output = command()
        .args(["trace", "validate"])
        .arg(&f.expected)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value(&output)["status"], "valid_observation_trace");
    assert_eq!(value(&output)["manifest"]["source"], "authored");
    assert_eq!(value(&output)["frames"], 2);
    let output = f.compare().output().unwrap();
    assert!(output.status.success());
    assert_eq!(value(&output)["status"], "equal_observations");
    assert_eq!(value(&output)["float_policy"], "exact_f32_bits");
    assert!(value(&output)["difference"].is_null());
    assert_eq!(fs::read(&f.expected).unwrap(), AUTHORED);
    assert_eq!(fs::read(&f.actual).unwrap(), AUTHORED);
    for args in [
        ["trace", "--help"].as_slice(),
        ["trace", "validate", "--help"].as_slice(),
        ["trace", "compare", "--help"].as_slice(),
    ] {
        assert!(command().args(args).output().unwrap().status.success());
    }
}

#[test]
fn divergence_is_structured_and_nonzero_with_explicit_tolerance() {
    let f = Fixture::new();
    f.mutate(|v| v["frames"][1]["state"]["position"][0] = 2.25.into());
    let output = f.compare().output().unwrap();
    assert!(!output.status.success());
    let report = value(&output);
    assert_eq!(report["status"], "diverged");
    assert_eq!(report["difference"]["tick"], 1);
    assert_eq!(report["difference"]["field"], "state.position[0]");
    assert!(
        report["difference"]["expected"]
            .as_str()
            .unwrap()
            .contains("0x")
    );
    let output = f
        .compare()
        .args(["--absolute-tolerance", "0.25"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(value(&output)["float_policy"], "absolute_tolerance");
    f.mutate(|v| v["frames"][1]["input"]["buttons"] = 2.into());
    let output = f
        .compare()
        .args(["--absolute-tolerance", "1000"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(value(&output)["difference"]["field"], "input.buttons");
    for bad in ["NaN", "inf", "-inf", "-1"] {
        assert!(
            !f.compare()
                .arg(format!("--absolute-tolerance={bad}"))
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn malformed_or_incompatible_traces_are_errors_not_gameplay_differences() {
    let f = Fixture::new();
    for change in [
        (|v: &mut Value| v["version"] = 2.into()) as fn(&mut Value),
        |v: &mut Value| v["manifest"]["rom_sha256"] = "wrong".into(),
        |v: &mut Value| v["manifest"]["state_boundary"] = "before tick".into(),
        |v: &mut Value| v["frames"][1]["tick"] = 9.into(),
        |v: &mut Value| v["unknown"] = true.into(),
        |v: &mut Value| v["frames"][1]["state"]["position"][0] = Value::Null,
    ] {
        f.mutate(change);
        let output = f.compare().output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    fs::write(&f.actual, b"{").unwrap();
    assert!(!f.compare().output().unwrap().status.success());
    fs::remove_file(&f.actual).unwrap();
    assert!(!f.compare().output().unwrap().status.success());
    assert_eq!(fs::read(&f.expected).unwrap(), AUTHORED);
}

#[test]
fn lengths_and_initial_observations_are_not_silently_ignored() {
    let f = Fixture::new();
    f.mutate(|v| {
        v["frames"].as_array_mut().unwrap().pop();
    });
    let output = f.compare().output().unwrap();
    assert!(!output.status.success());
    assert_eq!(value(&output)["difference"]["field"], "frames.length");
    f.mutate(|v| v["initial_state"]["face_yaw"] = 1.into());
    let output = f.compare().output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        value(&output)["difference"]["field"],
        "initial_state.face_yaw"
    );
}

#[test]
fn divergent_comparison_never_succeeds_when_stdout_is_closed() {
    let f = Fixture::new();
    f.mutate(|v| v["frames"][0]["state"]["action"] = 123.into());
    // A pre-closed reader forces a report-write failure without a scheduling race.
    for json in [true, false] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut command = command();
        command
            .args(["trace", "compare"])
            .arg(&f.expected)
            .arg(&f.actual);
        if json {
            command.arg("--json");
        }
        let output = command.stdout(Stdio::from(writer)).output().unwrap();
        assert!(
            !output.status.success(),
            "divergence must survive a broken output pipe"
        );
    }
}

#[test]
fn byte_limit_is_enforced_without_modifying_input() {
    let f = Fixture::new();
    fs::OpenOptions::new()
        .write(true)
        .open(&f.actual)
        .unwrap()
        .set_len(sm64_sim::reference::MAX_BYTES as u64 + 1)
        .unwrap();
    let output = f.compare().output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds"));
    assert_eq!(
        fs::metadata(&f.actual).unwrap().len(),
        sm64_sim::reference::MAX_BYTES as u64 + 1
    );
}
