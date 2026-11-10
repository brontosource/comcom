//! The work `comcom cmake` does differently for each CMake generator.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use db::Db;
use miette::bail;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;

use super::Cache;

mod make;
mod ninja;

pub(super) trait Generator {
  /// Has the build tool re-run CMake if its inputs changed since the last
  /// configure, so that the database CMake wrote is current.
  fn refresh(&self, build_dir: &Path) -> Result<()>;

  /// Builds the generated files (such as headers) that compilations in `db`
  /// need.
  fn build_generated_inputs(&self, build_dir: &Path, db: &Db) -> Result<()>;
}

/// The generator the build directory with this `cache` was configured for.
pub(super) fn from_cache(cache: &Cache) -> Result<Box<dyn Generator>> {
  let name: String = cache.require("CMAKE_GENERATOR")?;
  match name.as_str() {
    "Ninja" => Ok(Box::new(ninja::Ninja::new(
      cache.require::<PathBuf>("CMAKE_MAKE_PROGRAM")?,
    ))),
    "Unix Makefiles" => Ok(Box::new(make::Make::new(
      cache.require::<PathBuf>("CMAKE_MAKE_PROGRAM")?,
    ))),
    n => Err(miette!(
      help = "Reconfigure the build directory with `-G Ninja` or `-G \"Unix \
              Makefiles\"`",
      "CMake generator '{n}' is not supported"
    )),
  }
}

/// Runs the build tool `cmd` (named `tool` in errors) with its stdout sent to
/// comcom's stderr, so comcom's own stdout stays empty.
fn run_build(cmd: &mut Command, tool: &str) -> Result<()> {
  let status = cmd
    .stdin(Stdio::null())
    .stdout(std::io::stderr())
    .status()
    .into_diagnostic()
    .with_context(|| {
      format!("Failed to run {}", Path::new(cmd.get_program()).display())
    })?;
  if !status.success() {
    let why = match status.code() {
      Some(n) => format!("Exit {n}"),
      None => {
        #[cfg(unix)]
        {
          use std::os::unix::process::ExitStatusExt;
          if let Some(signal) = status.signal() {
            format!("Terminated by signal {signal}")
          } else {
            "Terminated by signal".to_string()
          }
        }
        #[cfg(not(unix))]
        {
          "Terminated by signal".to_string()
        }
      }
    };
    bail!("{tool} build failed ({why})");
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  /// `from_cache` for a build directory whose CMakeCache.txt has `lines`.
  fn generator_for(lines: &[&str]) -> Result<Box<dyn Generator>> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CMakeCache.txt"), lines.join("\n"))
      .unwrap();
    from_cache(&Cache::read_from(dir.path()).unwrap())
  }

  #[test]
  fn unix_makefiles_is_supported() {
    assert!(generator_for(&[
      "CMAKE_GENERATOR:INTERNAL=Unix Makefiles",
      "CMAKE_MAKE_PROGRAM:FILEPATH=/usr/bin/make",
    ])
    .is_ok());
  }

  #[test]
  fn unix_makefiles_needs_a_make_program() {
    let err = generator_for(&["CMAKE_GENERATOR:INTERNAL=Unix Makefiles"])
      .err()
      .unwrap();
    assert!(err.to_string().contains("CMAKE_MAKE_PROGRAM"), "{err}");
  }

  #[test]
  fn other_makefile_generators_are_not_supported() {
    let err = generator_for(&[
      "CMAKE_GENERATOR:INTERNAL=MinGW Makefiles",
      "CMAKE_MAKE_PROGRAM:FILEPATH=/usr/bin/make",
    ])
    .err()
    .unwrap();
    assert_eq!(
      err.to_string(),
      "CMake generator 'MinGW Makefiles' is not supported"
    );
  }
}
