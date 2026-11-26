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
      self.do_write(db, false)
    }
  }

  /// Like [`finish`](Self::finish), but before overwriting an output file
  /// whose contents change, moves it to `<output>.bak`.
  pub(crate) fn finish_with_backup(&self, db: Db) -> Result<()> {
    if self.check {
      self.do_check(db)
    } else {
      self.do_write(db, true)
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

  fn do_write(&self, db: Db, backup: bool) -> Result<()> {
    let path = self.output_path()?;
    let shown = path.display();
    let contents = db.render()?;

    if backup {
      if let Some(warning) = back_up(&path, &contents)? {
        warn(&warning);
      }
    }

    let was_symlink = path
      .symlink_metadata()
      .is_ok_and(|m| m.file_type().is_symlink());

    let message = match write_if_changed(&path, &contents)? {
      WriteOutcome::Written => {
        if was_symlink {
          // `write_if_changed` renames a temp file over `path`, which replaces
          // the link itself rather than writing through it.
          warn(&format!("Replaced symlink {shown} with a regular file"));
        }
        format!("Wrote {} entries to {shown}", db.len())
      }
      WriteOutcome::Unchanged => {
        format!("{shown} unchanged ({} entries)", db.len())
      }
    };
    advise(&message);
    Ok(())
  }
}

/// Moves `path` to `<path>.bak` if it exists and does not already hold
/// `contents`.
fn back_up(path: &Path, contents: &str) -> Result<Option<String>> {
  let Ok(metadata) = path.symlink_metadata() else {
    return Ok(None);
  };
  if is_up_to_date(path, contents)? {
    return Ok(None);
  }
  let shown = path.display();
  let bak = PathBuf::from(format!("{shown}.bak"));
  // Renaming moves a symlink itself, leaving its target alone.
  std::fs::rename(path, &bak)
    .into_diagnostic()
    .with_context(|| format!("cannot back up {shown}"))?;
  Ok(Some(if metadata.file_type().is_symlink() {
    format!(
      "Replaced symlink {shown} with a regular file; the symlink is now {}",
      bak.display()
    )
  } else {
    format!("Replaced {shown}; the previous version is in {}", bak.display())
  }))
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

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;

  /// An output file in a fresh directory, and the flags that write to it.
  fn setup(check: bool) -> (tempfile::TempDir, PathBuf, CommonArgs) {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("compile_commands.json");
    let args = CommonArgs {
      build_deps: false,
      output: Some(out.clone()),
      check,
    };
    (t, out, args)
  }

  fn bak(out: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", out.display()))
  }

  #[test]
  fn backup_when_contents_change() {
    let (_t, out, args) = setup(false);
    fs::write(&out, "old").unwrap();
    args.finish_with_backup(Db::new()).unwrap();
    assert_eq!(fs::read_to_string(bak(&out)).unwrap(), "old");
    assert_eq!(fs::read_to_string(&out).unwrap(), Db::new().render().unwrap());
  }

  #[test]
  fn no_backup_when_unchanged() {
    let (_t, out, args) = setup(false);
    fs::write(&out, Db::new().render().unwrap()).unwrap();
    args.finish_with_backup(Db::new()).unwrap();
    assert!(!bak(&out).exists());
  }

  #[test]
  fn no_backup_without_existing_output() {
    let (_t, out, args) = setup(false);
    args.finish_with_backup(Db::new()).unwrap();
    assert!(out.exists());
    assert!(!bak(&out).exists());
  }

  #[cfg(unix)]
  #[test]
  fn backup_of_symlink_keeps_target() {
    let (t, out, args) = setup(false);
    let target = t.path().join("target.json");
    fs::write(&target, "old").unwrap();
    std::os::unix::fs::symlink(&target, &out).unwrap();
    args.finish_with_backup(Db::new()).unwrap();
    assert_eq!(fs::read_to_string(bak(&out)).unwrap(), "old");
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    assert!(!out.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&out).unwrap(), Db::new().render().unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn backup_warning_names_a_replaced_symlink() {
    let (t, out, _) = setup(false);
    let target = t.path().join("target.json");
    fs::write(&target, "old").unwrap();
    std::os::unix::fs::symlink(&target, &out).unwrap();
    let warning = back_up(&out, "new").unwrap().unwrap();
    assert!(warning.contains("symlink"), "{warning}");
  }

  #[test]
  fn backup_warning_for_a_regular_file() {
    let (_t, out, _) = setup(false);
    fs::write(&out, "old").unwrap();
    let warning = back_up(&out, "new").unwrap().unwrap();
    assert!(!warning.contains("symlink"), "{warning}");
    assert!(warning.contains(".bak"), "{warning}");
  }

  #[test]
  fn check_never_backs_up() {
    let (_t, out, args) = setup(true);
    fs::write(&out, "old").unwrap();
    assert!(args.finish_with_backup(Db::new()).is_err());
    assert!(!bak(&out).exists());
    assert_eq!(fs::read_to_string(&out).unwrap(), "old");
  }

  #[test]
  fn plain_finish_never_backs_up() {
    let (_t, out, args) = setup(false);
    fs::write(&out, "old").unwrap();
    args.finish(Db::new()).unwrap();
    assert!(!bak(&out).exists());
  }
}
