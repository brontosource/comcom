//! The CMake backend: lets CMake refresh the `compile_commands.json` it writes
//! in a configured build directory, then normalizes it. Anything that depends
//! on the CMake generator is done by its [`generator::Generator`].
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use db::Db;
use miette::bail;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;
use miette::Severity;

use crate::BuildSystem;

mod cache;
pub use cache::Cache;

mod generator;

mod parse;

pub struct Cmake {
  pub build_dir: PathBuf,
  pub build_deps: bool,
  cache: Cache,
}

impl BuildSystem for Cmake {
  fn generate(&self) -> Result<Db> {
    let generator = generator::from_cache(&self.cache)?;
    generator.refresh(&self.build_dir)?;

    let db = self.db()?;
    if self.build_deps {
      generator.build_generated_inputs(&self.build_dir, &db)?;
    }
    Ok(db)
  }
}

impl Cmake {
  pub fn new(build_dir: PathBuf, build_deps: bool) -> Result<Cmake> {
    let cache = Cache::read_from(&build_dir)?;
    Ok(Cmake { build_dir, build_deps, cache })
  }

  /// The normalized database CMake wrote in the build directory.
  fn db(&self) -> Result<Db> {
    Db::read_from(&self.db_path()?)
  }

  /// The path of the `compile_commands.json` CMake wrote in the build
  /// directory, first reconfiguring with `cmake` and the export on if there is
  /// none.
  ///
  /// Whether the export is on is decided by whether the database exists, not
  /// by the cache: a project that turns it on with
  /// `set(CMAKE_EXPORT_COMPILE_COMMANDS ON)` (as cmake/comcom.cmake does)
  /// still gets a cache entry with an empty, false value. A database left
  /// behind after the export was turned off cannot be told apart by mtime:
  /// CMake 3.20.6 and 4.4.5 write build.ninja after compile_commands.json in
  /// the same generate step.
  fn db_path(&self) -> Result<PathBuf> {
    let path = self.build_dir.join("compile_commands.json");
    if path.is_file() {
      return Ok(path);
    }

    eprintln!(
      "{:?}",
      miette!(
        severity = Severity::Advice,
        "{} is missing; reconfiguring with CMAKE_EXPORT_COMPILE_COMMANDS=ON",
        path.display()
      )
    );

    let cmake = self.cmake_program()?;
    let show_cmd = || {
      format!(
        "{} -DCMAKE_EXPORT_COMPILE_COMMANDS=ON {}",
        cmake.display(),
        self.build_dir.display()
      )
    };
    let status = Command::new(&cmake)
      .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
      .arg(&self.build_dir)
      .stdin(Stdio::null())
      .stdout(std::io::stderr())
      .status()
      .into_diagnostic()
      .with_context(|| format!("Failed to run `{}`", show_cmd()))?;
    if !status.success() {
      bail!("`{}` failed ({status})", show_cmd());
    }

    if !path.is_file() {
      bail!("CMake did not write {}", path.display());
    }
    Ok(path)
  }

  /// The `cmake` the build directory was configured with.
  fn cmake_program(&self) -> Result<PathBuf> {
    let cmake: PathBuf = self.cache.require("CMAKE_COMMAND")?;
    if !cmake.exists() {
      bail!(
        "{} (CMAKE_COMMAND in CMakeCache.txt) does not exist; reconfigure the \
       build directory",
        cmake.display()
      );
    }
    Ok(cmake)
  }
}
