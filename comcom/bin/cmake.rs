use std::path::Path;
use std::path::PathBuf;

use build_system::cmake::Cmake;
use build_system::BuildSystem;
use miette::miette;
use miette::Result;

use crate::common::current_dir;
use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// The build directory (default: the one configured for this project).
  pub(crate) build_dir: Option<PathBuf>,
}

impl Args {
  pub(crate) fn build_system(&self) -> Result<Box<dyn BuildSystem>> {
    Ok(Box::new(Cmake {
      build_dir: self.find_build_dir()?,
      build_deps: self.common.build_deps,
    }))
  }

  fn find_build_dir(&self) -> Result<PathBuf> {
    let cwd = current_dir()?;
    self
      .build_dir
      .as_ref()
      .map(|dir| Ok(cwd.join(dir)))
      .or_else(|| configured_build_dir(&cwd))
      .or_else(|| configured_build_dir(&cwd.join("build")))
      .unwrap_or_else(|| {
        Err(miette!(
          "Unable to find a configured CMake build directory; pass one."
        ))
      })
  }
}

/// `dir`, if CMake has configured it.
fn configured_build_dir(dir: &Path) -> Option<Result<PathBuf>> {
  dir
    .join("CMakeCache.txt")
    .is_file()
    .then(|| Ok(dir.to_path_buf()))
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
