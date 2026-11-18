use std::ffi::OsString;
use std::path::Path;

use build_system::bazel::Bazel;
use build_system::cmake::Cmake;
use build_system::BuildSystem;
use clap::Parser;
use clap::Subcommand;
use miette::miette;
use miette::Result;

use crate::bazel;
use crate::cmake;
use crate::common::CommonArgs;
use crate::make;
use crate::trace;

/// Produces compile_commands.json for a build. Without a subcommand, attempts
/// to detect the build system.
#[derive(Parser)]
#[command(name = "comcom", version, args_conflicts_with_subcommands = true)]
pub(crate) struct Cli {
  #[command(subcommand)]
  pub(crate) cmd: Option<Cmd>,

  /// Flags for the detected build system when no subcommand is given.
  #[command(flatten)]
  pub(crate) common: CommonArgs,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
  /// Snapshot the compile commands of Bazel targets.
  Bazel(bazel::Args),
  /// Snapshot the compile commands of a configured CMake build directory.
  Cmake(cmake::Args),
  /// Run a `make` build and snapshot the compile commands it runs (Linux
  /// only).
  Make(make::Args),
  /// Run a build command and snapshot the compile commands it runs (Linux
  /// only). Replaces the output, backing up an existing one as `<output>.bak`.
  /// Note that tracing will only find the compile commands that actually run
  /// so you must use this command with a complete rebuild to ensure all
  /// commands are captured.
  Trace(trace::Args),
}

/// A build system `comcom` can detect: its subcommand name, whether a
/// directory is the root of a project using it, and the subcommand to run for
/// it, with its defaults and the given common flags.
type Detectable = (&'static str, fn(&Path) -> bool, fn(CommonArgs) -> Cmd);

const DETECTABLE: [Detectable; 2] = [
  ("bazel", Bazel::exists_in, |common| {
    Cmd::Bazel(bazel::Args::with_defaults(common))
  }),
  ("cmake", Cmake::exists_in, |common| {
    Cmd::Cmake(cmake::Args::with_defaults(common))
  }),
];

impl Cmd {
  /// The subcommand for the one build system `root` is the root of, run with
  /// `common`.
  pub(crate) fn detect(root: &Path, common: CommonArgs) -> Result<Cmd> {
    let mut found: Vec<&Detectable> = DETECTABLE
      .iter()
      .filter(|(_, exists_in, _)| exists_in(root))
      .collect();
    match found.len() {
      0 => Err(miette!(
        help = "Run from the root of a project, or name its build system, \
                e.g. `comcom cmake`",
        "no supported build system found in {}",
        root.display()
      )),
      1 => Ok((found.remove(0).2)(common)),
      _ => {
        let names: Vec<&str> = found.iter().map(|(name, ..)| *name).collect();
        Err(miette!(
          help = format!("Pass one, e.g. `comcom {}`", names[0]),
          "found several build systems in {}: {}",
          root.display(),
          names.join(", ")
        ))
      }
    }
  }
}

impl Cli {
  /// Parses argv, accepting negative target patterns among the positional
  /// patterns of `comcom bazel`.
  pub fn parse(mut args: Vec<OsString>) -> Result<Cli, clap::Error> {
    preprocess_cli(&mut args);
    let mut cli = Cli::try_parse_from(args)?;
    postprocess_cli(&mut cli);
    Ok(cli)
  }

  pub fn parse_or_exit() -> Cli {
    // TODO(6): prefer `.into_ok()` over `.unwrap()`. stabilizing on Dec 24,
    // 2026 in v1.101
    Self::parse(std::env::args_os().collect())
      .map_err(|e| e.exit())
      .unwrap()
  }

  #[cfg(test)]
  pub(crate) fn try_parse(args: &[&str]) -> Result<Self, clap::Error> {
    Self::parse(args.into_iter().map(Into::into).collect())
  }
}

/// Stands in for the leading `-` of a negative target pattern while clap
/// parses. It is a Unicode character reserved for private use, so it never
/// appears in real arguments.
const NEGATION_MARK: char = '\u{E000}';

/// Replaces the leading hyphen of each negative Bazel target pattern (`-//...`,
/// `-@repo//...`) with a [`NEGATION_MARK`]. Without this, clap would treat
/// them as flags and fail to parse. Only `comcom bazel` takes target
/// patterns, and everything after `--` goes to `bazel build` as is.
fn preprocess_cli(args: &mut [OsString]) {
  if args.get(1).is_none_or(|cmd| cmd != "bazel") {
    return;
  }
  for arg in args[2..].iter_mut().take_while(|arg| *arg != "--") {
    let Some(rest) = arg.to_str().and_then(|a| a.strip_prefix('-')) else {
      continue;
    };
    if rest.starts_with("//") || rest.starts_with('@') {
      *arg = format!("{NEGATION_MARK}{rest}").into();
    }
  }
}

/// Undoes [`preprocess_cli`]: puts back the hyphen wherever clap stored a
/// marked argument.
fn postprocess_cli(cli: &mut Cli) {
  let Some(Cmd::Bazel(args)) = &mut cli.cmd else {
    return;
  };
  for pattern in &mut args.patterns {
    unmark(pattern);
  }
  for path in [&mut args.common.output, &mut args.bazel]
    .into_iter()
    .flatten()
  {
    if let Some(mut s) = path.to_str().map(str::to_owned) {
      unmark(&mut s);
      *path = s.into();
    }
  }
}

fn unmark(arg: &mut String) {
  if let Some(rest) = arg.strip_prefix(NEGATION_MARK) {
    *arg = format!("-{rest}");
  }
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use super::*;

  fn detect(markers: &[&str]) -> Result<Cmd> {
    let t = tempfile::tempdir().unwrap();
    for marker in markers {
      fs::write(t.path().join(marker), "").unwrap();
    }
    let Cli { common, .. } =
      Cli::try_parse(&["comcom", "-o", "x.json"]).unwrap();
    Cmd::detect(t.path(), common)
  }

  #[test]
  fn no_subcommand_takes_common_flags() {
    let cli = Cli::try_parse(&["comcom", "--check", "-o", "x.json"]).unwrap();
    assert!(cli.cmd.is_none());
    assert!(cli.common.check && !cli.common.build_deps);
    assert_eq!(cli.common.output, Some(PathBuf::from("x.json")));
  }

  #[test]
  fn common_flags_go_after_the_subcommand() {
    assert!(Cli::try_parse(&["comcom", "--check", "cmake"]).is_err());
  }

  #[test]
  fn detects_bazel() {
    let Cmd::Bazel(a) = detect(&["MODULE.bazel"]).unwrap() else {
      panic!("not bazel");
    };
    assert!(a.patterns.is_empty() && a.bazel.is_none());
    assert_eq!(a.common.output, Some(PathBuf::from("x.json")));
  }

  #[test]
  fn detects_cmake() {
    let Cmd::Cmake(a) = detect(&["CMakeLists.txt"]).unwrap() else {
      panic!("not cmake");
    };
    assert!(a.build_dir.is_none());
    assert_eq!(a.common.output, Some(PathBuf::from("x.json")));
  }

  #[test]
  fn detects_none() {
    let Err(e) = detect(&["Makefile"]) else {
      panic!("detected a build system");
    };
    let e = format!("{e:?}");
    assert!(e.contains("no supported build system"), "{e}");
  }

  #[test]
  fn detects_several() {
    let Err(e) = detect(&["WORKSPACE", "CMakeLists.txt"]) else {
      panic!("detected one build system");
    };
    let e = format!("{e:?}");
    assert!(e.contains("several build systems"), "{e}");
    assert!(e.contains("bazel, cmake"), "{e}");
  }
}
