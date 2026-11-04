use std::process::ExitCode;

use miette::miette;
use miette::Result;

mod bazel;
mod cli;
mod cmake;
mod common;

fn main() -> ExitCode {
  initialize_miette();
  let cli = cli::Cli::parse_or_exit();

  match run(&cli.cmd) {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("{e:?}");
      ExitCode::from(1)
    }
  }
}

fn run(cmd: &cli::Cmd) -> Result<()> {
  let name = match cmd {
    cli::Cmd::Bazel(_) => "bazel",
    cli::Cmd::Cmake(_) => "cmake",
  };
  Err(miette!("`comcom {name}` is not implemented yet"))
}

fn initialize_miette() {
  // Errors print as miette's graphical reports. Long lines are left whole so
  // the paths and commands in them can be copied.
  miette::set_hook(Box::new(|_| {
    Box::new(miette::MietteHandlerOpts::new().wrap_lines(false).build())
  }))
  .expect("the report handler is set once");
}
