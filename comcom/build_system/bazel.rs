use std::path::Path;
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

/// Files whose presence marks the root of a Bazel workspace.
const WORKSPACE_MARKERS: [&str; 4] =
  ["MODULE.bazel", "REPO.bazel", "WORKSPACE.bazel", "WORKSPACE"];

impl BuildSystem for Bazel {
  fn exists_in(root: &Path) -> bool {
    WORKSPACE_MARKERS.iter().any(|m| root.join(m).is_file())
  }

  fn generate(&self) -> Result<Db> {
    Err(miette!("`comcom bazel` is not implemented yet"))
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;

  #[test]
  fn exists_with_any_workspace_marker() {
    for marker in WORKSPACE_MARKERS {
      let t = tempfile::tempdir().unwrap();
      fs::write(t.path().join(marker), "").unwrap();
      assert!(Bazel::exists_in(t.path()), "{marker}");
    }
  }

  #[test]
  fn does_not_exist_without_marker() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("BUILD.bazel"), "").unwrap();
    // A directory named like a marker is not a marker.
    fs::create_dir(t.path().join("WORKSPACE")).unwrap();
    assert!(!Bazel::exists_in(t.path()));
  }
}
