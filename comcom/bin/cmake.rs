use std::path::Path;
use std::path::PathBuf;

use build_system::cmake::Cache;
use build_system::cmake::Cmake;
use build_system::BuildSystem;
use miette::bail;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;

use crate::common::current_dir;
use crate::common::CommonArgs;

#[derive(clap::Args)]
pub(crate) struct Args {
  #[command(flatten)]
  pub(crate) common: CommonArgs,

  /// The build directory (default: the one configured for this project).
  pub(crate) build_dir: Option<PathBuf>,
}

impl Args {
  pub(crate) fn build_system(&self) -> Result<Box<dyn BuildSystem>> {
    Ok(Box::new(Cmake::new(self.find_build_dir()?, self.common.build_deps)?))
  }

  fn find_build_dir(&self) -> Result<PathBuf> {
    find_build_dir(&current_dir()?, self.build_dir.as_deref())
  }
}

/// The canonicalized source directory a build directory was configured for.
fn source_dir(build_dir: &Path) -> Result<PathBuf> {
  let source: PathBuf =
    Cache::read_from(build_dir)?.require("CMAKE_HOME_DIRECTORY")?;
  source
    .canonicalize()
    .into_diagnostic()
    .with_context(|| format!("resolving source directory {}", source.display()))
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
  let Ok(read) = std::fs::read_dir(dir) else {
    return Vec::new();
  };
  read
    .filter_map(|e| e.ok())
    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
    .map(|e| e.path())
    .filter(|p| p.is_dir())
    .collect()
}

/// Non-hidden subdirectories of `root` and of `root/out`, sorted.
fn candidates(root: &Path) -> Vec<PathBuf> {
  let mut all = subdirs(root);
  all.extend(subdirs(&root.join("out")));
  all.sort();
  all
}

/// The CMake build directory to use: `explicit` (relative to `root`) if given,
/// else the one configured build directory for the source tree `root`.
fn find_build_dir(root: &Path, explicit: Option<&Path>) -> Result<PathBuf> {
  if let Some(explicit) = explicit {
    let dir = root.join(explicit);
    if !dir.join("CMakeCache.txt").is_file() {
      bail!(
        "{} is not a configured CMake build directory (no CMakeCache.txt)",
        dir.display()
      );
    }
    return Ok(dir);
  }
  let want = root
    .canonicalize()
    .into_diagnostic()
    .with_context(|| format!("resolving {}", root.display()))?;
  let mut found: Vec<PathBuf> = candidates(root)
    .into_iter()
    .filter(|c| source_dir(c).is_ok_and(|s| s == want))
    .collect();
  match found.len() {
    0 => Err(miette!(
      "no configured CMake build directory found in {}; configure one with \
       `cmake -B build` or pass its path",
      root.display()
    )),
    1 => Ok(found.remove(0)),
    _ => {
      let rel: Vec<String> = found
        .iter()
        .map(|p| p.strip_prefix(root).unwrap_or(p).display().to_string())
        .collect();
      Err(miette!(
        "found several configured CMake build directories:\n  {}\npass one, \
         e.g. `comcom cmake {}`",
        rel.join("\n  "),
        rel[0]
      ))
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::cli::Cli;
  use crate::cli::Cmd;

  fn fake_cmake(dir: &Path, home: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
      dir.join("CMakeCache.txt"),
      format!("CMAKE_HOME_DIRECTORY:INTERNAL={}\n", home.display()),
    )
    .unwrap();
  }

  fn root() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let r = t.path().canonicalize().unwrap();
    (t, r)
  }

  fn err(r: Result<PathBuf>) -> String {
    format!("{:#}", r.unwrap_err())
  }

  fn parse(args: &[&str]) -> Args {
    match Cli::try_parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}")) {
      Cli { cmd: Cmd::Cmake(a) } => a,
      _ => panic!("not cmake"),
    }
  }

  #[test]
  fn cmake_args() {
    let a = parse(&["comcom", "cmake", "build", "--check", "-o", "x.json"]);
    assert_eq!(a.build_dir, Some(PathBuf::from("build")));
    assert_eq!(a.common.output, Some(PathBuf::from("x.json")));
    assert!(a.common.check && !a.common.build_deps);
    let a = parse(&["comcom", "cmake", "--build-deps"]);
    assert_eq!(a.build_dir, None);
    assert!(a.common.build_deps);
  }

  #[test]
  fn cmake_has_no_negative_patterns() {
    assert!(Cli::try_parse(&["comcom", "cmake", "-//x"]).is_err());
    assert!(Cli::try_parse(&["comcom", "cmake", "build", "-//x"]).is_err());
    // A build directory named `bazel` is not the `bazel` subcommand.
    assert!(Cli::try_parse(&["comcom", "cmake", "bazel", "-//x"]).is_err());
  }

  #[test]
  fn detect_single() {
    let (_t, r) = root();
    fake_cmake(&r.join("build"), &r);
    assert_eq!(find_build_dir(&r, None).unwrap(), r.join("build"));
  }

  #[test]
  fn detect_build_star_and_out_preset() {
    let (_t, r) = root();
    fake_cmake(&r.join("out/debug"), &r);
    assert_eq!(find_build_dir(&r, None).unwrap(), r.join("out/debug"));
  }

  #[test]
  fn detect_several_errors() {
    let (_t, r) = root();
    fake_cmake(&r.join("build-debug"), &r);
    fake_cmake(&r.join("build-release"), &r);
    let e = err(find_build_dir(&r, None));
    for s in [
      "several configured",
      "build-debug",
      "build-release",
      "comcom cmake build-debug",
    ] {
      assert!(e.contains(s), "{s} not in {e}");
    }
  }

  #[test]
  fn detect_none_errors() {
    let (_t, r) = root();
    let e = err(find_build_dir(&r, None));
    assert!(e.contains("no configured CMake build directory"), "{e}");
    assert!(e.contains("cmake -B build"), "{e}");
  }

  #[test]
  fn detect_ignores_other_source_tree() {
    let (_t, r) = root();
    fake_cmake(&r.join("build"), &r.join("elsewhere"));
    assert!(err(find_build_dir(&r, None)).contains("no configured CMake"));
  }

  #[test]
  fn detect_ignores_hidden_dirs() {
    let (_t, r) = root();
    fake_cmake(&r.join(".cache"), &r);
    assert!(err(find_build_dir(&r, None)).contains("no configured CMake"));
  }

  #[test]
  fn detect_through_symlinked_root() {
    let (_t, r) = root();
    let real = r.join("real");
    fake_cmake(&real.join("build"), &real);
    let link = r.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let found = find_build_dir(&link, None).unwrap();
    assert_eq!(found.canonicalize().unwrap(), real.join("build"));
  }

  #[test]
  fn explicit_dir_used_even_for_other_tree() {
    let (_t, r) = root();
    fake_cmake(&r.join("b"), &r.join("elsewhere"));
    assert_eq!(find_build_dir(&r, Some(Path::new("b"))).unwrap(), r.join("b"));
  }

  #[test]
  fn explicit_dir_not_configured_errors() {
    let (_t, r) = root();
    fs::create_dir(r.join("b")).unwrap();
    let e = err(find_build_dir(&r, Some(Path::new("b"))));
    assert!(e.contains("not a configured CMake build directory"), "{e}");
  }
}
