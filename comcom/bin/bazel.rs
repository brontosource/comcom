use std::path::PathBuf;

use build_system::bazel::Bazel;
use build_system::BuildSystem;
use miette::miette;
use miette::Result;

use crate::common::find_program;
use crate::common::program_from_env;
use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// Target patterns to include (default: //...); `-//my-target/...` excludes
  /// the target.
  pub(crate) patterns: Vec<String>,
  /// The Bazel binary (default: $COMCOM_BAZEL, else bazel, else bazelisk).
  #[arg(long)]
  pub(crate) bazel: Option<PathBuf>,
  /// Extra arguments for `bazel build`, after `--`.
  #[arg(last = true)]
  pub(crate) bazel_args: Vec<String>,
}

impl Args {
  /// The arguments of a bare `comcom bazel`, with `common` flags.
  pub(crate) fn with_defaults(common: CommonArgs) -> Args {
    Args {
      common,
      patterns: Vec::new(),
      bazel: None,
      bazel_args: Vec::new(),
    }
  }

  pub(crate) fn build_system(&self) -> Result<Box<dyn BuildSystem>> {
    let patterns = if self.patterns.is_empty() {
      vec!["//...".to_string()]
    } else {
      self.patterns.clone()
    };

    Ok(Box::new(Bazel {
      bazel: self.find_program()?,
      patterns,
      build_deps: self.common.build_deps,
      extra_args: self.bazel_args.clone(),
    }))
  }

  fn find_program(&self) -> Result<PathBuf> {
    self
      .bazel
      .clone()
      .map(|bazel| Ok(bazel))
      .or_else(|| program_from_env("COMCOM_BAZEL"))
      .or_else(|| find_program("bazel"))
      .or_else(|| find_program("bazelisk"))
      .unwrap_or_else(|| Err(miette!("Unable to find `bazel` implementation.")))
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;
  use crate::cli::Cli;
  use crate::cli::Cmd;

  fn parse(args: &[&str]) -> Args {
    match Cli::try_parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}")) {
      Cli { cmd: Some(Cmd::Bazel(a)), .. } => a,
      _ => panic!("not bazel"),
    }
  }

  #[test]
  fn negative_patterns() {
    let a = parse(&[
      "comcom",
      "bazel",
      "//...",
      "-//nocc/...",
      "-@repo//x",
      "-o",
      "out.json",
      "--check",
      "--build-deps",
      "--",
      "-c",
      "opt",
    ]);
    assert_eq!(a.patterns, ["//...", "-//nocc/...", "-@repo//x"]);
    assert_eq!(a.common.output, Some(PathBuf::from("out.json")));
    assert!(a.common.check && a.common.build_deps);
    assert_eq!(a.bazel_args, ["-c", "opt"]);
  }

  #[test]
  fn flags_before_and_between_patterns() {
    let a = parse(&[
      "comcom",
      "bazel",
      "--check",
      "//a",
      "-o",
      "o.json",
      "-//b",
      "--build-deps",
    ]);
    assert_eq!(a.patterns, ["//a", "-//b"]);
    assert_eq!(a.common.output, Some(PathBuf::from("o.json")));
    assert!(a.common.check);
    assert!(a.common.build_deps);
    assert!(a.bazel_args.is_empty());
  }

  #[test]
  fn unknown_flags_still_rejected() {
    assert!(Cli::try_parse(&["comcom", "bazel", "//a", "--nope"]).is_err());
    assert!(Cli::try_parse(&["comcom", "bazel", "//a", "-c", "opt"]).is_err());
  }

  #[test]
  fn option_values_and_bazel_args_untouched() {
    let a = parse(&["comcom", "bazel", "//a", "--bazel", "-@b", "-o", "-//c"]);
    assert_eq!(a.patterns, ["//a"]);
    assert_eq!(a.bazel, Some(PathBuf::from("-@b")));
    assert_eq!(a.common.output, Some(PathBuf::from("-//c")));
    let a = parse(&["comcom", "bazel", "//a", "--", "-//c", "--", "-@d"]);
    assert_eq!(a.patterns, ["//a"]);
    assert_eq!(a.bazel_args, ["-//c", "--", "-@d"]);
  }
}
