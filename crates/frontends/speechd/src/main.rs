#![forbid(unsafe_code)]

mod engine;

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let voices_dir = match engine::voices_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("sd_dengjen: {error}");
            return ExitCode::FAILURE;
        }
    };
    let stdin = io::stdin().lock();
    let stdout = io::stdout().lock();
    match dengjen_speechd::run(engine::Engine::new(voices_dir), stdin, stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sd_dengjen: {error}");
            ExitCode::FAILURE
        }
    }
}
