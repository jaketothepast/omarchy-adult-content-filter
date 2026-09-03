use anyhow::Result;
use clap::{Parser, Subcommand};

#[cfg(test)]
use clap::CommandFactory;

#[derive(Parser)]
#[command(name = "omarchy-kids-browser-filter")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Infer,
    Bench,
    Run,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Doctor => anyhow::bail!("doctor is not implemented"),
        Command::Infer => anyhow::bail!("infer is not implemented"),
        Command::Bench => anyhow::bail!("bench is not implemented"),
        Command::Run => anyhow::bail!("run is not implemented"),
    }
}

#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}
