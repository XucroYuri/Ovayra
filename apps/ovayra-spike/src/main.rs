use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ovayra-spike", version, about = "Ovayra Phase 0 proof runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
}

fn main() {
    match Cli::parse().command {
        Command::Version => println!("ovayra-spike {}", env!("CARGO_PKG_VERSION")),
    }
}
