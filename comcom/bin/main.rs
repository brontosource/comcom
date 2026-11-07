use std::process::ExitCode;

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
  let (build_system, common) = match cmd {
    cli::Cmd::Bazel(args) => (args.build_system()?, &args.common),
    cli::Cmd::Cmake(args) => (args.build_system()?, &args.common),
  };
  common.finish(build_system.generate()?)
}

fn initialize_miette() {
  // Errors print as miette's graphical reports. Long lines are left whole so
  // the paths and commands in them can be copied.
  miette::set_hook(Box::new(|_| {
    Box::new(miette::MietteHandlerOpts::new().wrap_lines(false).build())
  }))
  .expect("the report handler is set once");
}
