use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use db::Db;
use miette::Result;
use output::advise;

use crate::cmake::generator::run_build;
use crate::cmake::generator::Generator;

/// The `Unix Makefiles` generator. Holds the `make` program CMake configured.
pub(super) struct Make {
  make: PathBuf,
}

impl Generator for Make {
  /// `cmake_check_build_system` is the target of CMake's top-level Makefile
  /// that every other target runs first; it re-runs CMake if its inputs
  /// changed.
  fn refresh(&self, build_dir: &Path) -> Result<()> {
    self.build(build_dir, &["cmake_check_build_system"])
  }

  /// CMake's Makefiles cannot build only the generated files: a custom
  /// command's rule (in its target's `build.make`) names the tools it runs as
  /// plain files, which only the target's full `all` rule (in `Makefile2`)
  /// builds first, along with everything else that target depends on. So this
  /// is a full build.
  fn build_generated_inputs(&self, build_dir: &Path, _db: &Db) -> Result<()> {
    advise(
      "Generating build inputs requires a full build (Unix Makefiles \
       generator)",
    );
    self.build(build_dir, &[])
  }
}

impl Make {
  pub(super) fn new(make: PathBuf) -> Make {
    Make { make }
  }

  fn build(&self, build_dir: &Path, targets: &[&str]) -> Result<()> {
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    run_build(
      Command::new(&self.make)
        .arg("-C")
        .arg(build_dir)
        .arg(format!("-j{jobs}"))
        .args(targets),
      "Make",
    )
  }
}

#[cfg(test)]
mod tests {
  use std::os::unix::fs::PermissionsExt;

  use super::*;

  /// A fake `make` in a temporary directory that records its arguments, one
  /// per line, in `args` beside it and then exits with `code`.
  struct FakeMake {
    dir: tempfile::TempDir,
  }

  impl FakeMake {
    fn new(code: i32) -> FakeMake {
      let dir = tempfile::tempdir().unwrap();
      let script = dir.path().join("make");
      std::fs::write(
        &script,
        format!(
          "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args\"\n\
           exit {code}\n"
        ),
      )
      .unwrap();
      std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap();
      FakeMake { dir }
    }

    fn make(&self) -> Make {
      Make::new(self.dir.path().join("make"))
    }

    /// The arguments of the last run.
    fn args(&self) -> Vec<String> {
      std::fs::read_to_string(self.dir.path().join("args"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
    }
  }

  fn jobs() -> String {
    let n = std::thread::available_parallelism().map_or(1, |n| n.get());
    format!("-j{n}")
  }

  #[test]
  fn refresh_runs_cmake_check_build_system() {
    let fake = FakeMake::new(0);
    fake.make().refresh(Path::new("/some/build")).unwrap();
    assert_eq!(
      fake.args(),
      ["-C", "/some/build", &jobs(), "cmake_check_build_system"]
    );
  }

  #[test]
  fn build_generated_inputs_builds_everything() {
    let fake = FakeMake::new(0);
    fake
      .make()
      .build_generated_inputs(Path::new("/some/build"), &Db::new())
      .unwrap();
    assert_eq!(fake.args(), ["-C", "/some/build", &jobs()]);
  }

  #[test]
  fn build_dir_with_spaces_is_one_argument() {
    let fake = FakeMake::new(0);
    fake.make().refresh(Path::new("/my project/build")).unwrap();
    assert_eq!(fake.args()[..2], ["-C", "/my project/build"]);
  }

  #[test]
  fn failing_make_is_an_error() {
    let fake = FakeMake::new(2);
    let err = fake.make().refresh(Path::new("/some/build")).unwrap_err();
    assert_eq!(err.to_string(), "Make build failed (Exit 2)");
  }

  #[test]
  fn missing_make_is_an_error() {
    let make = Make::new(PathBuf::from("/nonexistent/make"));
    let err = make.refresh(Path::new("/some/build")).unwrap_err();
    assert_eq!(err.to_string(), "Failed to run /nonexistent/make");
  }
}
