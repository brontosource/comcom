use std::path::PathBuf;

use db::Db;
use miette::miette;
use miette::Result;

use crate::BuildSystem;

pub struct Bazel {
  pub bazel: PathBuf,
  pub patterns: Vec<String>,
  pub build_deps: bool,
  pub extra_args: Vec<String>,
}

impl BuildSystem for Bazel {
  fn generate(&self) -> Result<Db> {
    Err(miette!("`comcom bazel` is not implemented yet"))
  }
}
