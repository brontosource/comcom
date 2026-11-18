use std::path::Path;

use db::Db;

pub mod bazel;
pub mod cmake;
mod exit;
pub mod make;

pub trait BuildSystem {
  /// Whether `root` is the top of a project that uses this build system.
  fn exists_in(root: &Path) -> bool
  where
    Self: Sized;

  fn generate(&self) -> miette::Result<Db>;
}
