//! The plain `make` backend. On Linux it runs the build under `ptrace` (see
//! [`crate::ptrace`]); elsewhere it is unsupported.

use std::path::Path;
use std::path::PathBuf;

use db::Db;
use miette::Result;

#[cfg(target_os = "linux")]
use crate::ptrace::warnings::Warnings;
use crate::BuildSystem;

pub struct Make {
  /// The `make` program.
  pub make: PathBuf,
  /// Extra arguments for `make`.
  pub args: Vec<String>,
}

impl BuildSystem for Make {
  fn exists_in(root: &Path) -> bool {
    ["GNUmakefile", "makefile", "Makefile"]
      .iter()
      .any(|name| root.join(name).is_file())
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
      "`comcom make` is only supported on Linux"
    ))
  }
}

impl Make {
  /// Rebuilds everything with `make -B`, tracing it, and returns the compiles
  /// it ran along with warnings about any that may be missing.
  #[cfg(target_os = "linux")]
  fn snapshot(&self) -> Result<(Db, Warnings)> {
    use std::process::Command;
    use std::process::Stdio;

    let mut cmd = Command::new(&self.make);
    // Make only runs the compiles of targets that are out of date, so all of
    // them are rebuilt.
    cmd
      .arg("-B")
      .args(&self.args)
      .stdin(Stdio::null())
      // Keeps comcom's own stdout empty.
      .stdout(std::io::stderr());
    crate::ptrace::snapshot(cmd, "Make build")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exists_with_a_makefile() {
    for name in ["Makefile", "makefile", "GNUmakefile"] {
      let t = tempfile::tempdir().unwrap();
      std::fs::write(t.path().join(name), "all:\n").unwrap();
      assert!(Make::exists_in(t.path()), "{name}");
    }
  }

  #[test]
  fn does_not_exist_without_a_makefile() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("Makefile")).unwrap();
    assert!(!Make::exists_in(t.path()));
  }

  #[cfg(not(target_os = "linux"))]
  #[test]
  fn is_unsupported_off_linux() {
    let make = Make {
      make: PathBuf::from("make"),
      args: Vec::new(),
    };
    let err = make.generate().unwrap_err();
    assert_eq!(err.to_string(), "`comcom make` is only supported on Linux");
  }

  #[cfg(target_os = "linux")]
  mod linux {
    use super::*;

    /// A project in a temporary directory with `files` (path, contents).
    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
      let t = tempfile::tempdir().unwrap();
      for (path, contents) in files {
        let path = t.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
      }
      t
    }

    fn make_in(dir: &Path) -> Make {
      Make {
        make: PathBuf::from("make"),
        args: vec!["-C".into(), dir.display().to_string()],
      }
    }

    /// (directory relative to `root`, file) of each entry.
    fn files(db: &Db, root: &Path) -> Vec<(String, String)> {
      let root = root.canonicalize().unwrap();
      db.entries()
        .iter()
        .map(|e| {
          let dir = e.directory.strip_prefix(&root).unwrap();
          (dir.display().to_string(), e.file.display().to_string())
        })
        .collect()
    }

    const MAIN: &str = "int main(void) { return 0; }\n";
    const LIB: &str = "int lib(void) { return 1; }\n";

    fn sample() -> tempfile::TempDir {
      project(&[
        (
          "Makefile",
          "all: prog sub\n\
           prog: main.o util.o\n\t$(CC) main.o util.o -o prog\n\
           %.o: %.c\n\t$(CC) -c $< -o $@\n\
           sub:\n\t$(MAKE) -C sub\n\
           .PHONY: sub\n",
        ),
        ("main.c", MAIN),
        ("util.c", LIB),
        // Run through `sh`: running a script this process just wrote can fail
        // with "Text file busy" when another test thread starts a process.
        ("sub/Makefile", "lib.o: lib.c\n\tsh ./build.sh\n"),
        // A script that clears the environment still cannot hide its compile.
        (
          "sub/build.sh",
          "#!/bin/sh\nexec env -i /usr/bin/cc -c lib.c -o lib.o\n",
        ),
        ("sub/lib.c", LIB),
      ])
    }

    #[test]
    fn records_every_compile() {
      let t = sample();
      let (db, warnings) = make_in(t.path()).snapshot().unwrap();
      assert_eq!(
        files(&db, t.path()),
        [
          ("sub".into(), "lib.c".into()),
          ("".into(), "main.c".into()),
          ("".into(), "util.c".into()),
        ]
      );
      assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn up_to_date_build_is_rebuilt() {
      let t = sample();
      let first = make_in(t.path()).snapshot().unwrap().0;
      let second = make_in(t.path()).snapshot().unwrap().0;
      assert_eq!(first, second);
      assert_eq!(second.len(), 3);
    }

    #[test]
    fn failing_build_is_an_error() {
      let t = project(&[("Makefile", "all:\n\texit 3\n")]);
      let err = make_in(t.path()).snapshot().unwrap_err();
      assert_eq!(err.to_string(), "Make build failed (Exit 2)");
    }

    #[test]
    fn program_left_running_is_a_warning() {
      let t = project(&[(
        "Makefile",
        "all:\n\t/bin/sleep 3 </dev/null >/dev/null 2>&1 &\n",
      )]);
      let (_, warnings) = make_in(t.path()).snapshot().unwrap();
      assert_eq!(warnings.len(), 1, "{warnings:?}");
      assert!(
        warnings.in_order()[0].contains("/bin/sleep 3"),
        "{}",
        warnings.in_order()[0]
      );
    }
  }
}
