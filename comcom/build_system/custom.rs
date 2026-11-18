//! A build system comcom does not know: a command the user supplies, run under
//! `ptrace` (see [`crate::ptrace`]) to record the compilations it runs.
//! `comcom` cannot make an arbitrary command rebuild everything, so the user
//! runs a full build. Linux only.

use std::path::Path;
use std::path::PathBuf;

use db::Db;
use miette::Result;

#[cfg(target_os = "linux")]
use crate::ptrace::warnings::Warnings;
use crate::BuildSystem;

pub struct Custom {
  /// The build command's program.
  pub program: PathBuf,
  /// Its arguments.
  pub args: Vec<String>,
}

impl BuildSystem for Custom {
  /// A bespoke build is never detected.
  fn exists_in(_root: &Path) -> bool {
    false
  }

  #[cfg(target_os = "linux")]
  fn generate(&self) -> Result<Db> {
    let (db, warnings) = self.snapshot()?;
    for w in warnings.in_order() {
      output::warn(w);
    }
    Ok(db)
  }

  #[cfg(not(target_os = "linux"))]
  fn generate(&self) -> Result<Db> {
    Err(miette::miette!(
      help = "comcom needs to see every program a build runs, which only \
              Linux allows without special privileges",
      "comcom tracing is only supported on Linux"
    ))
  }
}

impl Custom {
  /// Runs the build command, tracing it, and returns the compilations it ran
  /// along with warnings about any that may be missing.
  #[cfg(target_os = "linux")]
  fn snapshot(&self) -> Result<(Db, Warnings)> {
    use std::process::Command;
    use std::process::Stdio;

    let mut cmd = Command::new(&self.program);
    cmd
      .args(&self.args)
      .stdin(Stdio::null())
      // Keeps comcom's own stdout empty.
      .stdout(std::io::stderr());
    let (db, mut warnings) = crate::ptrace::snapshot(cmd, "Build command")?;
    if db.is_empty() {
      warnings.push(
        "the build command ran no compilations; was the build already up to date?",
      );
    }
    Ok((db, warnings))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn never_detected() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("build.sh"), "").unwrap();
    assert!(!Custom::exists_in(t.path()));
  }

  #[cfg(not(target_os = "linux"))]
  #[test]
  fn is_unsupported_off_linux() {
    let custom = Custom {
      program: PathBuf::from("sh"),
      args: Vec::new(),
    };
    let err = custom.generate().unwrap_err();
    assert_eq!(err.to_string(), "comcom tracing is only supported on Linux");
  }

  #[cfg(target_os = "linux")]
  mod linux {
    use super::*;

    /// Runs `script` with `sh -c` in `dir`.
    fn sh_in(dir: &Path, script: &str) -> Custom {
      Custom {
        program: PathBuf::from("sh"),
        args: vec!["-c".into(), format!("cd '{}' && {script}", dir.display())],
      }
    }

    #[test]
    fn records_every_compile() {
      let t = tempfile::tempdir().unwrap();
      std::fs::write(t.path().join("a.c"), "int a(void) { return 0; }\n")
        .unwrap();
      std::fs::write(t.path().join("b.c"), "int b(void) { return 1; }\n")
        .unwrap();
      // Run through `sh`: running a script this process just wrote can fail
      // with "Text file busy" when another test thread starts a process.
      std::fs::write(
        t.path().join("build.sh"),
        "cc -c a.c -o a.o && cc -c b.c -o b.o\n",
      )
      .unwrap();
      let (db, warnings) = sh_in(t.path(), "sh build.sh").snapshot().unwrap();
      let files: Vec<String> = db
        .entries()
        .iter()
        .map(|e| e.file.display().to_string())
        .collect();
      assert_eq!(files, ["a.c", "b.c"]);
      assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn failing_command_is_an_error() {
      let t = tempfile::tempdir().unwrap();
      let err = sh_in(t.path(), "exit 3").snapshot().unwrap_err();
      assert_eq!(err.to_string(), "Build command failed (Exit 3)");
    }

    #[test]
    fn no_compiles_is_a_warning() {
      let t = tempfile::tempdir().unwrap();
      let (db, warnings) = sh_in(t.path(), "true").snapshot().unwrap();
      assert_eq!(db.len(), 0);
      assert_eq!(
        warnings.in_order(),
        ["the build command ran no compilations; was the build already up to \
          date?"]
      );
    }

    #[test]
    fn missing_program_is_an_error() {
      let custom = Custom {
        program: PathBuf::from("/nonexistent/build"),
        args: Vec::new(),
      };
      let err = custom.snapshot().unwrap_err();
      assert!(err.to_string().contains("/nonexistent/build"), "{err}");
    }
  }
}
