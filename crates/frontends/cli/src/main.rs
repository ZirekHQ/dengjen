#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod speak;
mod voices;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "dengjen", author, version, about, long_about = None, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Synthesize speech from text
    Speak(speak::SpeakArgs),
    /// Manage installed voices
    Voices {
        #[command(flatten)]
        location: voices::Location,
        #[command(subcommand)]
        command: Option<voices::VoicesCommand>,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return report_parse_error(&error),
    };
    match cli.command {
        Command::Speak(args) => match speak::run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::from(1)
            }
        },
        Command::Voices { location, command } => voices::run(&location, command),
    }
}

fn looks_like_legacy_config(error: &clap::Error) -> bool {
    error.kind() == ErrorKind::InvalidSubcommand
        && matches!(
            error.get(ContextKind::InvalidSubcommand),
            Some(ContextValue::String(arg))
                if arg.contains(['.', '/', '\\']) || std::path::Path::new(arg).exists()
        )
}

fn report_parse_error(error: &clap::Error) -> ExitCode {
    let _ = error.print();
    if looks_like_legacy_config(error) {
        eprintln!("tip: synthesis moved to a subcommand; run `dengjen speak <config>`");
    }
    ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2))
}
