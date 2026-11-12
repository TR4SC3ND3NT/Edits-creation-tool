use edits_gui::{parse_args, run, Invocation, USAGE};
use std::process::ExitCode;

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Invocation::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Invocation::Run(project, options)) => match run(project, options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("error: {e:#}");
            eprintln!("usage: edits-gui <project.json> [options]   (edits-gui --help lists them)");
            ExitCode::from(2)
        }
    }
}
