//! Native, bounded flat-walk execution, independent of ROM/reference outputs.
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use sm64_sim::movement::Scenario;
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run explicit initialization/authored inputs; emit a native Trace v1 to stdout.
    /// This is a bounded headless slice, not general gameplay or terrain collision.
    Run { scenario: PathBuf },
}

pub fn run(command: Command) -> Result<()> {
    let Command::Run { scenario } = command;
    let file = File::open(&scenario)
        .with_context(|| format!("opening movement scenario {}", scenario.display()))?;
    let mut data = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut data)?;
    if data.len() > 1024 * 1024 {
        bail!("movement scenario exceeds 1 MiB");
    }
    let scenario: Scenario = serde_json::from_slice(&data).context("reading movement scenario")?;
    let trace = scenario.run().context("running bounded flat-walk slice")?;
    // Only write after the COMPLETE scenario succeeds. No partial traces on
    // unsupported input/state. The command never owns or truncates an output path.
    let mut stdout = std::io::stdout().lock();
    trace.write(&mut stdout)?;
    writeln!(stdout)?;
    Ok(())
}
