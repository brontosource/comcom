use std::path::PathBuf;

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
