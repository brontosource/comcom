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
  use super::*;

  /// The `Make` for the fake `make` script `name` in testdata. The scripts
  /// are checked in rather than written by the tests: running a script this
  /// process just wrote can fail with "Text file busy" when another test
  /// thread starts a process meanwhile, since that process briefly inherits
  /// the open file.
  fn fake(name: &str) -> Make {
    Make::new(
      std::env::current_dir()
        .unwrap()
        .join("comcom/build_system/cmake/generator/testdata")
        .join(name),
    )
  }

  /// A build directory for `fake_make.sh`, which records its arguments there.
  struct FakeMake {
    dir: tempfile::TempDir,
  }

  impl FakeMake {
    fn new() -> FakeMake {
      FakeMake { dir: tempfile::tempdir().unwrap() }
    }

    fn make(&self) -> Make {
      fake("fake_make.sh")
    }

    fn build_dir(&self) -> &Path {
      self.dir.path()
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
    let fake = FakeMake::new();
    fake.make().refresh(fake.build_dir()).unwrap();
    let dir = fake.build_dir().display().to_string();
    assert_eq!(fake.args(), ["-C", &dir, &jobs(), "cmake_check_build_system"]);
  }

  #[test]
  fn build_generated_inputs_builds_everything() {
    let fake = FakeMake::new();
    fake
      .make()
      .build_generated_inputs(fake.build_dir(), &Db::new())
      .unwrap();
    let dir = fake.build_dir().display().to_string();
    assert_eq!(fake.args(), ["-C", &dir, &jobs()]);
  }

  #[test]
  fn build_dir_with_spaces_is_one_argument() {
    let fake = FakeMake::new();
    let build = fake.build_dir().join("my build");
    std::fs::create_dir(&build).unwrap();
    fake.make().refresh(&build).unwrap();
    let args = std::fs::read_to_string(build.join("args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args[..2], ["-C", &build.display().to_string()]);
  }

  #[test]
  fn failing_make_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("failing_make.sh").refresh(dir.path()).unwrap_err();
    assert_eq!(err.to_string(), "Make build failed (Exit 2)");
  }

  #[test]
  fn missing_make_is_an_error() {
    let make = Make::new(PathBuf::from("/nonexistent/make"));
    let err = make.refresh(Path::new("/some/build")).unwrap_err();
    assert_eq!(err.to_string(), "Failed to run /nonexistent/make");
  }
}
