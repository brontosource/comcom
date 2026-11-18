use std::path::PathBuf;

use build_system::custom::Custom;
use build_system::BuildSystem;
use miette::miette;
use miette::Result;

use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// The build command and its arguments, after `--`. It should build
  /// everything: only the compiles it runs are recorded.
  #[arg(last = true, required = true)]
  pub(crate) command: Vec<String>,
}

impl Args {
  pub(crate) fn build_system(&self) -> Result<Box<dyn BuildSystem>> {
    if self.common.build_deps {
      return Err(miette!(
        help = "Have your build command build them",
        "`--build-deps` has no effect with `comcom trace`"
      ));
    }
    Ok(Box::new(self.custom()))
  }

  fn custom(&self) -> Custom {
    // clap requires at least one value.
    let (program, args) = self.command.split_first().expect("a command");
    Custom {
      program: PathBuf::from(program),
      args: args.to_vec(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cli::Cli;
  use crate::cli::Cmd;

  fn parse(args: &[&str]) -> Args {
    match Cli::try_parse(args).unwrap().cmd {
      Some(Cmd::Trace(a)) => a,
      _ => panic!("not `comcom trace`"),
    }
  }

  #[test]
  fn command_and_arguments_are_passed() {
    let c = parse(&["comcom", "trace", "--", "./build.sh", "-j4"]).custom();
    assert_eq!(c.program, PathBuf::from("./build.sh"));
    assert_eq!(c.args, ["-j4"]);
  }

  #[test]
  fn command_is_required() {
    assert!(Cli::try_parse(&["comcom", "trace"]).is_err());
  }

  #[test]
  fn common_flags_are_accepted() {
    let a =
      parse(&["comcom", "trace", "--check", "-o", "out.json", "--", "sh"]);
    assert!(a.common.check);
    assert_eq!(a.common.output, Some(PathBuf::from("out.json")));
  }

  #[test]
  fn build_deps_is_rejected() {
    let a = parse(&["comcom", "trace", "--build-deps", "--", "sh"]);
    let Err(e) = a.build_system() else {
      panic!("accepted --build-deps");
    };
    assert!(e.to_string().contains("--build-deps"), "{e}");
  }
}
