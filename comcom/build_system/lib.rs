use db::Db;

pub mod bazel;
pub mod cmake;

pub trait BuildSystem {
  fn generate(&self) -> miette::Result<Db>;
}
