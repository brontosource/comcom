use std::path::PathBuf;

use db::Db;
use miette::miette;
use miette::IntoDiagnostic;
use miette::Result;
use miette::WrapErr;

/// Flags shared by every subcommand.
#[derive(clap::Args)]
pub(crate) struct CommonArgs {
  /// Also build the generated files (e.g. headers) the compiles need.
  #[arg(long)]
  pub(crate) build_deps: bool,
  /// Output path (default: ./compile_commands.json).
  #[arg(short, long)]
  pub(crate) output: Option<PathBuf>,
  /// Do not write; exit 1 if the output file is out of date.
  #[arg(long)]
  pub(crate) check: bool,
}

impl CommonArgs {
  /// Writes (or, with `check`, compares) the output file for `db`.
  pub(crate) fn finish(&self, _db: Db) -> Result<()> {
    Err(miette!("CommonArgs::finish is not yet implemented."))
  }
}

pub(crate) fn current_dir() -> Result<PathBuf> {
  std::env::current_dir()
    .into_diagnostic()
    .context("cannot determine the current directory")
}

/// Whether `program` is a file in some directory on `$PATH`.
fn on_path(program: &str) -> bool {
  std::env::var_os("PATH").is_some_and(|paths| {
    std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
  })
}

pub(crate) fn program_from_env(name: &str) -> Option<Result<PathBuf>> {
  Some(Ok(PathBuf::from(std::env::var_os(name)?)))
}

pub(crate) fn find_program(name: &str) -> Option<Result<PathBuf>> {
  if on_path(name) {
    Some(Ok(PathBuf::from(name)))
  } else {
    None
  }
}
