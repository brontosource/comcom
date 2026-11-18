use std::path::PathBuf;

use build_system::make::Make;
use build_system::BuildSystem;
use miette::Result;

use crate::common::program_from_env;
use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// The make binary (default: $COMCOM_MAKE, else make).
  #[arg(long)]
  pub(crate) make: Option<PathBuf>,
  /// Extra arguments for `make`, after `--`.
  #[arg(last = true)]
  pub(crate) make_args: Vec<String>,
}

impl Args {
  pub(crate) fn build_system(&self) -> Result<Box<dyn BuildSystem>> {
    Ok(Box::new(self.make_system()?))
  }

  fn make_system(&self) -> Result<Make> {
    let make = match self.make.clone() {
      Some(make) => make,
      None => program_from_env("COMCOM_MAKE")
        .unwrap_or_else(|| Ok(PathBuf::from("make")))?,
    };
    Ok(Make { make, args: self.make_args.clone() })
  }
}

#[cfg(test)]
mod tests {
  use build_system::make::Make;

  use super::*;
  use crate::cli::Cli;
  use crate::cli::Cmd;

  fn parse(args: &[&str]) -> Args {
    match Cli::try_parse(args).unwrap().cmd {
      Some(Cmd::Make(a)) => a,
      _ => panic!("not `comcom make`"),
    }
  }

  fn make_of(a: &Args) -> Make {
    a.make_system().unwrap()
  }

  #[test]
  fn bare_make_runs_make() {
    let m = make_of(&parse(&["comcom", "make"]));
    assert_eq!(m.make, PathBuf::from("make"));
    assert!(m.args.is_empty());
  }

  #[test]
  fn make_and_its_arguments_are_passed() {
    let a =
      parse(&["comcom", "make", "--make", "/opt/gmake", "--", "-j4", "V=1"]);
    let m = make_of(&a);
    assert_eq!(m.make, PathBuf::from("/opt/gmake"));
    assert_eq!(m.args, ["-j4", "V=1"]);
  }

  #[test]
  fn common_flags_are_accepted() {
    let a = parse(&["comcom", "make", "--check", "-o", "out.json"]);
    assert!(a.common.check);
  }
}
