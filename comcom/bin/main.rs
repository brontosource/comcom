use std::process::ExitCode;

use miette::Result;

mod bazel;
mod cli;
mod cmake;
mod common;
mod make;
mod trace;

fn main() -> ExitCode {
  initialize_miette();
  let cli = cli::Cli::parse_or_exit();

  match run(cli) {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("{e:?}");
      ExitCode::from(1)
    }
  }
}

fn run(cli: cli::Cli) -> Result<()> {
  let cmd = match cli.cmd {
    Some(cmd) => cmd,
    None => cli::Cmd::detect(&common::current_dir()?, cli.common)?,
  };
  let (build_system, common) = match &cmd {
    cli::Cmd::Bazel(args) => (args.build_system()?, &args.common),
    cli::Cmd::Cmake(args) => (args.build_system()?, &args.common),
    cli::Cmd::Make(args) => (args.build_system()?, &args.common),
    cli::Cmd::Trace(args) => {
      // Traced builds record only the compilations that were run. If a build
      // system doesn't run a compilation because it was already cached, we may
      // miss pre-existing compilations. As such, we don't want to delete
      // an existing output file. Doing so with a caching build system would
      // mean that runing `comcom` twice would result in an empty
      // `compile_commands.json` file.
      return args
        .common
        .finish_with_backup(args.build_system()?.generate()?);
    }
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
