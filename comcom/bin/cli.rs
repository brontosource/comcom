use std::ffi::OsString;

use clap::Parser;
use clap::Subcommand;

use crate::bazel;
use crate::cmake;

/// Produces compile_commands.json for a build.
#[derive(Parser)]
#[command(name = "comcom", version)]
pub(crate) struct Cli {
  #[command(subcommand)]
  pub(crate) cmd: Cmd,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
  /// Snapshot the compile commands of Bazel targets.
  Bazel(bazel::Args),
  /// Snapshot the compile commands of a configured CMake build directory.
  Cmake(cmake::Args),
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
    match Self::parse(std::env::args_os().collect()) {
      Ok(cli) => cli,
      Err(e) if !e.use_stderr() => {
        // Used for --help and --version.
        e.exit();
      }
      Err(e) => {
        eprintln!("{e:#}");
        std::process::exit(2);
      }
    }
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
  let Cmd::Bazel(args) = &mut cli.cmd else {
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
