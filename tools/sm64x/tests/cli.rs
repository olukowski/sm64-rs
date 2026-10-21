//! End-to-end tests use only authored bytes and temporary directories.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    rom: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "sm64x-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let rom = dir.join("intentionally-wrong-extension.n64");
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        bytes[0x20..0x34].copy_from_slice(b"AUTHORED TEST ROM   ");
        bytes[0x3b..0x3f].copy_from_slice(b"NXTE");
        // One literal-only MIO0 block at canonical 0x40, plus zero padding.
        bytes[64..68].copy_from_slice(b"MIO0");
        bytes[68..72].copy_from_slice(&3u32.to_be_bytes());
        bytes[72..76].copy_from_slice(&17u32.to_be_bytes());
        bytes[76..80].copy_from_slice(&17u32.to_be_bytes());
        bytes[80] = 0xe0;
        bytes[81..84].copy_from_slice(b"abc");
        fs::write(&rom, bytes).unwrap();
        Self { dir, rom }
    }

    fn command(&self) -> Command {
        let mut command = bare_command();
        command.arg("--rom").arg(&self.rom).current_dir(&self.dir);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn bare_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sm64x"));
    command.env_remove("SM64_ROM");
    command
}

fn json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn info_is_json_and_does_not_trust_filename_or_title() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.rom).unwrap();
    let value = json(fixture.command().args(["info", "--json"]).output().unwrap());
    assert_eq!(value["source_byte_order"], "big_endian");
    assert_eq!(value["size_bytes"], 128);
    assert_eq!(value["header"]["title"], "AUTHORED TEST ROM");
    assert!(value["identity"]["known_revision"].is_null());
    assert_eq!(value["identity"]["sha1"].as_str().unwrap().len(), 40);
    assert_eq!(value["identity"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(fs::read(&fixture.rom).unwrap(), original);
}

#[test]
fn byte_swapped_source_has_same_identity_and_offsets() {
    let fixture = Fixture::new();
    let canonical = json(fixture.command().args(["info", "--json"]).output().unwrap());
    let mut bytes = fs::read(&fixture.rom).unwrap();
    for pair in bytes.as_chunks_mut::<2>().0 {
        pair.reverse();
    }
    fs::write(&fixture.rom, bytes).unwrap();
    let swapped = json(fixture.command().args(["info", "--json"]).output().unwrap());
    assert_eq!(swapped["source_byte_order"], "byte_swapped");
    assert_eq!(swapped["identity"], canonical["identity"]);
    let inventory = json(
        fixture
            .command()
            .args(["mio0", "list", "--validate", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(inventory["candidate_count"], 1);
    assert_eq!(inventory["valid_count"], 1);
    assert_eq!(inventory["entries"][0]["offset"], 64);
}

#[test]
fn can_select_rom_using_environment() {
    let fixture = Fixture::new();
    let value = json(
        bare_command()
            .env("SM64_ROM", &fixture.rom)
            .args(["info", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(value["size_bytes"], 128);
}

#[test]
fn explicit_rom_overrides_environment() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .env("SM64_ROM", fixture.dir.join("missing"))
        .arg("info")
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn validates_and_decodes_without_overwriting() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.rom).unwrap();
    let report = json(
        fixture
            .command()
            .args(["mio0", "list", "--validate", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(report["valid_count"], 1);
    assert_eq!(report["entries"][0]["decoded_size"], 3);
    let decoded = json(
        fixture
            .command()
            .args(["mio0", "decode", "0x40", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(decoded["decoded_size"], 3);
    let output = fixture.dir.join(".cache/mio0/00000040.bin");
    assert_eq!(fs::read(&output).unwrap(), b"abc");
    let repeated = fixture
        .command()
        .args(["mio0", "decode", "64"])
        .output()
        .unwrap();
    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("will not overwrite"));
    assert_eq!(fs::read(output).unwrap(), b"abc");
    let overwrite_rom = fixture
        .command()
        .args(["mio0", "decode", "64", "--out"])
        .arg(&fixture.rom)
        .output()
        .unwrap();
    assert!(!overwrite_rom.status.success());
    assert_eq!(fs::read(&fixture.rom).unwrap(), before);
}

#[test]
fn rejects_invalid_offsets_and_truncated_roms() {
    let fixture = Fixture::new();
    for offset in ["0x1000", "0", "-1", "99999999999999999999999999999"] {
        let output = fixture
            .command()
            .args(["mio0", "decode", offset])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    fs::write(&fixture.rom, [0x80, 0x37]).unwrap();
    let output = fixture.command().arg("info").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("too short"));
}

#[test]
fn missing_rom_has_actionable_error_and_help_needs_no_rom() {
    let output = bare_command().arg("info").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SM64_ROM"));
    assert!(
        bare_command()
            .arg("--help")
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn output_limit_rejects_before_creating_a_file() {
    let fixture = Fixture::new();
    let mut bytes = fs::read(&fixture.rom).unwrap();
    bytes[68..72].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(&fixture.rom, bytes).unwrap();
    let output = fixture
        .command()
        .args(["mio0", "decode", "64"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!fixture.dir.join(".cache").exists());
}

#[test]
fn oversized_sparse_rom_is_rejected() {
    let fixture = Fixture::new();
    fs::OpenOptions::new()
        .write(true)
        .open(&fixture.rom)
        .unwrap()
        .set_len(sm64_rom::MAX_ROM_SIZE as u64 + 1)
        .unwrap();
    let output = fixture.command().arg("info").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("safety limit"));
}

#[cfg(unix)]
#[test]
fn symlink_output_cannot_modify_rom() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.rom).unwrap();
    let output_path = fixture.dir.join("alias.bin");
    std::os::unix::fs::symlink(&fixture.rom, &output_path).unwrap();
    let output = fixture
        .command()
        .args(["mio0", "decode", "64", "--out"])
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(&fixture.rom).unwrap(), before);
}

#[test]
fn early_closing_output_consumer_does_not_cause_a_panic() {
    let fixture = Fixture::new();
    let mut bytes = fs::read(&fixture.rom).unwrap();
    let block = bytes[64..84].to_vec();
    bytes.extend(block.repeat(4096));
    fs::write(&fixture.rom, bytes).unwrap();
    for json in [false, true] {
        let mut command = fixture.command();
        command.args(["mio0", "list"]);
        if json {
            command.arg("--json");
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Close the reader, like `sm64x ... | head -n 1` after head exits.
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
    }
}

// A local ROM is optional and is never a fixture or a CI prerequisite.
#[test]
#[ignore = "needs a user-supplied US revision 0 ROM via SM64_ROM"]
fn local_us_reference_rom() {
    let Some(path) = std::env::var_os("SM64_ROM") else {
        eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
        return;
    };
    let path = Path::new(&path);
    let rom = sm64_rom::Rom::open(path).unwrap();
    assert_eq!(
        rom.identity().known_revision,
        Some(sm64_rom::KnownRevision::UsRevision0)
    );
    let mut count = 0;
    for (offset, window) in rom.bytes().windows(4).enumerate() {
        if window == b"MIO0" {
            let decoded = sm64_mio0::decode(&rom.bytes()[offset..], 16 * 1024 * 1024)
                .unwrap_or_else(|error| panic!("MIO0 candidate 0x{offset:08x}: {error}"));
            assert!(!decoded.bytes.is_empty());
            count += 1;
        }
    }
    // Observed count of decodable signatures, NOT a complete asset inventory.
    assert_eq!(count, 79);
}
