//! Read-only ROM inspection; extraction always creates a new output file.

mod collision;
mod level;
mod mio0;
mod movement;
mod trace;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sm64_rom::{ByteOrder, Header, Identity, Rom};

#[derive(Parser)]
#[command(
    version,
    about = "Inspect a user-supplied Super Mario 64 ROM (read-only)"
)]
struct Cli {
    /// ROM path; byte order is detected from content, not the extension.
    #[arg(long, global = true, env = "SM64_ROM", value_name = "PATH")]
    rom: Option<PathBuf>,
    /// Emit structured JSON instead of human-readable output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show canonical hashes, exact revision recognition, and N64 header fields.
    Info,
    /// Inspect course scripts and static collision (US revision 0 only).
    Level {
        #[arg(value_parser = ["bob"])]
        course: String,
        /// Decode static geometry/display lists/textures and report metadata (no files written).
        #[arg(long)]
        visuals: bool,
    },
    /// Probe static geometry (diagnostic, NOT original-game collision rules).
    Collision(collision::Options),
    /// Run the bounded headless flat-walk slice without a ROM.
    Movement {
        #[command(subcommand)]
        command: movement::Command,
    },
    /// Validate/compare observation traces without a ROM or gameplay model.
    Trace {
        #[command(subcommand)]
        command: trace::Command,
    },
    /// Inspect MIO0 signature candidates or decode a block.
    Mio0 {
        #[command(subcommand)]
        command: Mio0Command,
    },
}

#[derive(Subcommand)]
enum Mio0Command {
    /// Scan the whole ROM for MIO0 signatures (not an authoritative asset table).
    List {
        /// Decode candidates to check stream structure, not just headers.
        #[arg(long)]
        validate: bool,
        /// Per-block decoded-size limit; scan also has a 256 MiB aggregate budget.
        #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u64).range(1..=64))]
        max_output_mib: u64,
    },
    /// Decode one block at a canonical ROM offset into a NEW local file.
    Decode {
        /// Decimal or 0x-prefixed hexadecimal offset in canonical big-endian bytes.
        #[arg(value_parser = parse_offset)]
        offset: usize,
        /// Output file; defaults to .cache/mio0/OFFSET.bin. Never overwrites.
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Maximum decoded size in MiB (1–64), checked before allocation.
        #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u64).range(1..=64))]
        max_output_mib: u64,
    },
}

fn parse_offset(value: &str) -> std::result::Result<usize, String> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.map_err(|_| format!("invalid offset {value:?}: use decimal or 0x-prefixed hexadecimal"))
}

#[derive(Serialize)]
struct InfoReport<'a> {
    size_bytes: usize,
    source_byte_order: ByteOrder,
    canonical_byte_order: ByteOrder,
    header: &'a Header,
    identity: &'a Identity,
    warnings: Vec<&'static str>,
}

fn info(rom: &Rom, json: bool) -> Result<()> {
    let mut warnings = vec![
        "CRC1/CRC2 are stored header values; N64 boot/checksum verification is not implemented.",
    ];
    if rom.identity().known_revision.is_none() {
        warnings.push(
            "Unknown revision: generic inspection only; no US-specific asset mapping is enabled.",
        );
    }
    let report = InfoReport {
        size_bytes: rom.bytes().len(),
        source_byte_order: rom.source_order(),
        canonical_byte_order: ByteOrder::BigEndian,
        header: rom.header(),
        identity: rom.identity(),
        warnings,
    };
    if json {
        return print_json(&report);
    }
    let header = rom.header();
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "Title:          {}", header.title)?;
    writeln!(stdout, "Size:           {} bytes", report.size_bytes)?;
    writeln!(stdout, "Source order:   {}", rom.source_order())?;
    writeln!(
        stdout,
        "Canonical:      big-endian (.z64); all hashes/offsets use this order"
    )?;
    match rom.identity().known_revision {
        Some(revision) => writeln!(stdout, "Recognized:     {revision}")?,
        None => writeln!(stdout, "Recognized:     unknown ROM revision")?,
    }
    writeln!(stdout, "SHA-1:          {}", rom.identity().sha1)?;
    writeln!(stdout, "SHA-256:        {}", rom.identity().sha256)?;
    writeln!(stdout, "Game code:      {}", header.game_code)?;
    writeln!(
        stdout,
        "Country:        0x{:02x} ({})",
        header.country_code,
        header.country_name()
    )?;
    writeln!(stdout, "Revision byte:  {}", header.revision)?;
    writeln!(stdout, "Entry point:    0x{:08x}", header.entry_point)?;
    writeln!(stdout, "Clock rate:     0x{:08x}", header.clock_rate)?;
    writeln!(stdout, "Release:        0x{:08x}", header.release)?;
    writeln!(
        stdout,
        "CRC1 / CRC2:    {:08x} / {:08x} (stored, not verified)",
        header.crc1, header.crc2
    )?;
    for warning in report.warnings {
        writeln!(stdout, "Note: {warning}")?;
    }
    Ok(())
}

#[derive(Serialize)]
struct DecodeReport {
    offset: usize,
    decoded_size: usize,
    encoded_size: usize,
    output: PathBuf,
}

fn decode(rom: &Rom, offset: usize, out: Option<PathBuf>, limit: usize, json: bool) -> Result<()> {
    let input = rom
        .bytes()
        .get(offset..)
        .context("offset lies outside the ROM")?;
    let decoded = sm64_mio0::decode(input, limit)
        .with_context(|| format!("could not decode MIO0 at 0x{offset:08x}"))?;
    let output = out.unwrap_or_else(|| PathBuf::from(format!(".cache/mio0/{offset:08x}.bin")));
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    // create_new refuses existing files AND symlinks: never overwrite the input,
    // another ROM, or a previous extraction, even if paths alias one another.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .with_context(|| {
            format!(
                "could not create new output {} (will not overwrite)",
                output.display()
            )
        })?;
    if let Err(error) = file.write_all(&decoded.bytes).and_then(|()| file.flush()) {
        drop(file);
        let _ = fs::remove_file(&output);
        return Err(error).context("could not write decoded output");
    }
    let report = DecodeReport {
        offset,
        decoded_size: decoded.bytes.len(),
        encoded_size: decoded.consumed,
        output,
    };
    if json {
        print_json(&report)
    } else {
        writeln!(
            std::io::stdout().lock(),
            "Decoded 0x{offset:08x}: {} bytes from {} encoded bytes -> {}",
            report.decoded_size,
            report.encoded_size,
            report.output.display()
        )?;
        Ok(())
    }
}

fn print_json(value: &impl Serialize) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let command = match cli.command {
        Command::Trace { command } => return trace::run(command, cli.json),
        Command::Movement { command } => return movement::run(command),
        command => command,
    };
    let Some(path) = cli.rom else {
        bail!("no ROM selected; pass --rom PATH or set SM64_ROM (no ROM is bundled or downloaded)");
    };
    let rom = Rom::open(&path).with_context(|| format!("could not inspect {}", path.display()))?;
    match command {
        Command::Info => info(&rom, cli.json),
        Command::Collision(options) => collision::run(&rom, options, cli.json),
        Command::Trace { .. } | Command::Movement { .. } => {
            unreachable!("ROM-free commands handled before ROM loading")
        }
        Command::Level { course: _, visuals } => {
            let course = sm64_level::load_bob(&rom)?;
            level::report(&course, cli.json, visuals)
        }
        Command::Mio0 { command } => match command {
            Mio0Command::List {
                validate,
                max_output_mib,
            } => {
                let report =
                    mio0::inventory(rom.bytes(), validate, max_output_mib as usize * 1024 * 1024);
                if cli.json {
                    print_json(&report)
                } else {
                    report.print(&mut std::io::stdout().lock())?;
                    Ok(())
                }
            }
            Mio0Command::Decode {
                offset,
                out,
                max_output_mib,
            } => decode(
                &rom,
                offset,
                out,
                max_output_mib as usize * 1024 * 1024,
                cli.json,
            ),
        },
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Inspection output may be intentionally discarded by `head`. Comparisons
    // instead promise a reliable failure status: a report-write error must not
    // mask a detected divergence (or otherwise claim successful comparison).
    let allow_broken_pipe = !matches!(
        &cli.command,
        Command::Trace {
            command: trace::Command::Compare { .. }
        } | Command::Movement { .. }
    );
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let broken_pipe = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
                || error
                    .downcast_ref::<serde_json::Error>()
                    .is_some_and(|error| {
                        error.io_error_kind() == Some(std::io::ErrorKind::BrokenPipe)
                    });
            if broken_pipe && allow_broken_pipe {
                return ExitCode::SUCCESS;
            }
            let _ = writeln!(std::io::stderr().lock(), "error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_accept_decimal_and_hex_and_reject_overflow() {
        assert_eq!(parse_offset("1234"), Ok(1234));
        assert_eq!(parse_offset("0x4d2"), Ok(1234));
        assert_eq!(parse_offset("0X4D2"), Ok(1234));
        for value in ["-1", "0x", "potato", "18446744073709551616000"] {
            assert!(parse_offset(value).is_err());
        }
    }
}
