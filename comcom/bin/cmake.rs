use std::path::PathBuf;

use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// The build directory (default: the one configured for this project).
  pub(crate) build_dir: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cli::Cli;
  use crate::cli::Cmd;

  fn parse(args: &[&str]) -> Args {
    match Cli::try_parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}")) {
      Cli { cmd: Cmd::Cmake(a) } => a,
      _ => panic!("not cmake"),
    }
  }

  #[test]
  fn cmake_args() {
    let a = parse(&["comcom", "cmake", "build", "--check", "-o", "x.json"]);
    assert_eq!(a.build_dir, Some(PathBuf::from("build")));
    assert_eq!(a.common.output, Some(PathBuf::from("x.json")));
    assert!(a.common.check && !a.common.build_deps);
    let a = parse(&["comcom", "cmake", "--build-deps"]);
    assert_eq!(a.build_dir, None);
    assert!(a.common.build_deps);
  }

  #[test]
  fn cmake_has_no_negative_patterns() {
    assert!(Cli::try_parse(&["comcom", "cmake", "-//x"]).is_err());
    assert!(Cli::try_parse(&["comcom", "cmake", "build", "-//x"]).is_err());
    // A build directory named `bazel` is not the `bazel` subcommand.
    assert!(Cli::try_parse(&["comcom", "cmake", "bazel", "-//x"]).is_err());
  }
}
