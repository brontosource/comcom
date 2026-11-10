//! The work `comcom cmake` does differently for each CMake generator.

use std::path::Path;
use std::path::PathBuf;

use db::Db;
use miette::miette;
use miette::Result;

use super::Cache;

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
    n => Err(miette!(
      help = "Reconfigure the build directory with `-G Ninja`",
      "CMake generator '{n}' is not supported"
    )),
  }
}
