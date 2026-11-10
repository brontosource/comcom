use std::path::PathBuf;

use db::Db;
use miette::miette;
use miette::Result;

use crate::BuildSystem;

pub struct Cmake {
  pub build_dir: PathBuf,
  pub build_deps: bool,
}

impl BuildSystem for Cmake {
  fn generate(&self) -> Result<Db> {
    Err(miette!("`comcom cmake` is not implemented yet"))
  }
}
