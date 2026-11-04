use std::path::Path;
use std::path::PathBuf;

use db::Db;
use miette::bail;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;
use output::advise;
use output::is_up_to_date;
use output::warn;
use output::write_if_changed;
use output::WriteOutcome;

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
  fn output_path(&self) -> Result<PathBuf> {
    Ok(
      current_dir()?.join(
        self
          .output
          .as_deref()
          .unwrap_or(Path::new("compile_commands.json")),
      ),
    )
  }

  /// Writes (or, with `check`, compares) the output file for `db`.
  pub(crate) fn finish(&self, db: Db) -> Result<()> {
    if self.check {
      self.do_check(db)
    } else {
      self.do_write(db)
    }
  }

  fn do_check(&self, db: Db) -> Result<()> {
    let path = self.output_path()?;
    let contents = db.render()?;
    let shown = path.display();

    if is_up_to_date(&path, &contents)? {
      advise(&format!("{shown} is up to date"));
      Ok(())
    } else {
      bail!(
        help = "Run without `--check` to update it",
        "{shown} is out of date"
      );
    }
  }

  fn do_write(&self, db: Db) -> Result<()> {
    let path = self.output_path()?;
    let shown = path.display();

    if path
      .symlink_metadata()
      .is_ok_and(|m| m.file_type().is_symlink())
    {
      // Replace the link rather than write through it.
      warn(&format!("replacing symlink {shown} with a regular file"));
      std::fs::remove_file(&path)
        .into_diagnostic()
        .with_context(|| format!("failed to remove {shown}"))?;
    }

    let contents = db.render()?;
    let message = match write_if_changed(&path, &contents)? {
      WriteOutcome::Written => format!("wrote {} entries to {shown}", db.len()),
      WriteOutcome::Unchanged => {
        format!("{shown} unchanged ({} entries)", db.len())
      }
    };
    advise(&message);
    Ok(())
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
