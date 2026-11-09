//! Port of src/Ricochet.Cli/Program.cs (ADR 0091). This build has the `run` command only: it runs a Workspace's
//! Requests and checks, for CI. The other commands (export, env, docs, zip, clone) are not ported yet, and the C# CLI
//! still has them. The exit codes match the C# CLI: 0 passed, 1 a Request or check failed, 2 the run could not happen.

mod run;

use ricochet_core::collection_runner::EXIT_CANNOT_RUN;

const USAGE: &str = "Ricochet: run a workspace's requests and check their assertions.

Usage:
  ricochet run [folder] [options]

Only the run command is in this build. Run `ricochet run --help` for its options and exit codes.
The export, env, docs, zip and clone commands are not ported yet. Use the C# CLI for them.

Exit codes:
  0  Everything passed, or help was shown.
  1  A request or assertion failed, in any data row.
  2  The run could not happen, for example because a workspace file or the data file is unreadable.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(dispatch(&args));
}

fn dispatch(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None => {
            println!("{USAGE}");
            EXIT_CANNOT_RUN
        }
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            0
        }
        Some("run") => run::run_command(&args[1..]),
        Some(other @ ("export" | "env" | "docs" | "zip" | "clone")) => {
            eprintln!("\"ricochet {other}\" is not ported to the Rust CLI yet. Use the C# CLI for it.");
            EXIT_CANNOT_RUN
        }
        Some(other) => {
            eprintln!("Unknown command \"{other}\". Try: ricochet --help");
            EXIT_CANNOT_RUN
        }
    }
}
