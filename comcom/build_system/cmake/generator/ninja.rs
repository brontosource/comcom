use std::path::Path;
use std::path::PathBuf;

use db::Db;
use miette::miette;
use miette::Result;

use crate::cmake::generator::Generator;

/// The `Ninja` generator. Holds the `ninja` program CMake configured.
pub(super) struct Ninja {
  #[allow(dead_code)] // Not used until Ninja is implemented.
  ninja: PathBuf,
}

impl Generator for Ninja {
  fn refresh(&self, _build_dir: &Path) -> Result<()> {
    Err(miette!("refreshing a Ninja build directory is not implemented yet"))
  }

  fn build_generated_inputs(&self, _build_dir: &Path, _db: &Db) -> Result<()> {
    Err(miette!("building generated inputs with Ninja is not implemented yet"))
  }
}

impl Ninja {
  pub(super) fn new(ninja: PathBuf) -> Ninja {
    Ninja { ninja }
  }
}
