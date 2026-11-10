use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use db::Db;
use miette::bail;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;
use output::advise;
use output::warn;
use semver::SemVer;

use crate::cmake::generator::run_build;
use crate::cmake::generator::Generator;

/// The `Ninja` generator. Holds the `ninja` program CMake configured.
pub(super) struct Ninja {
  ninja: PathBuf,
}

impl Generator for Ninja {
  fn refresh(&self, build_dir: &Path) -> Result<()> {
    self.build(build_dir, &["build.ninja".to_string()])
  }

  fn build_generated_inputs(&self, build_dir: &Path, db: &Db) -> Result<()> {
    if self.supports_inputs_query() {
      advise("Building generated files (via cmake - ninja)");
      self.build_only_generated(build_dir, db)
    } else {
      advise(
        "Generating build inputs requires a full rebuild (ninja version < \
         1.13)",
      );
      self.build_all(build_dir)
    }
  }
}

impl Ninja {
  pub(super) fn new(ninja: PathBuf) -> Ninja {
    Ninja { ninja }
  }

  /// Builds `targets` (in chunks, to stay under ARG_MAX), doing nothing when
  /// there are none.
  fn build(&self, build_dir: &Path, targets: &[String]) -> Result<()> {
    for chunk in chunk_by_len(targets, MAX_ARGS_BYTES) {
      self.build_chunk(build_dir, chunk)?;
    }
    Ok(())
  }

  /// A plain full build (`ninja` with no targets).
  fn build_all(&self, build_dir: &Path) -> Result<()> {
    self.build_chunk(build_dir, &[])
  }

  /// Runs `ninja -C <build_dir> <targets>` with Ninja's stdout sent to
  /// comcom's stderr, so comcom's own stdout stays empty. With no targets this
  /// builds Ninja's defaults.
  fn build_chunk(&self, build_dir: &Path, targets: &[String]) -> Result<()> {
    run_build(
      Command::new(&self.ninja)
        .arg("-C")
        .arg(build_dir)
        .args(targets),
      "Ninja",
    )
  }

  fn version(&self) -> Result<SemVer> {
    let out = Command::new(&self.ninja)
      .arg("--version")
      .stdin(Stdio::null())
      .output()
      .into_diagnostic()
      .with_context(|| format!("Failed to run {}", self.ninja.display()))?;
    if !out.status.success() {
      bail!(
        "{} --version failed: {}",
        self.ninja.display(),
        String::from_utf8_lossy(&out.stderr).trim()
      );
    }
    String::from_utf8_lossy(&out.stdout).parse()
  }

  /// Whether this Ninja can answer `-t inputs -E -0`: those options first
  /// appear in Ninja 1.13.0. A Ninja whose version cannot be determined is
  /// treated as too old.
  fn supports_inputs_query(&self) -> bool {
    self.version().is_ok_and(|v| v >= SemVer::new(1, 13, 0))
  }

  /// Builds the custom-command outputs the compilations in `db` need.
  fn build_only_generated(&self, build_dir: &Path, db: &Db) -> Result<()> {
    let objects = objects(db, build_dir);
    let mapped = self.build_generated(build_dir, &objects)?;
    if mapped == 0 && !db.is_empty() {
      warn(
        "Could not map any database entry to a Ninja target; \
         no generated files built",
      );
    }
    Ok(())
  }

  /// Builds the custom-command outputs needed by the compilations of `objects`.
  /// `-t inputs` rejects unknown targets, so objects Ninja does not know about
  /// are dropped first. Returns how many objects Ninja knew. Runs `ninja -t
  /// targets all` once.
  fn build_generated(&self, dir: &Path, objects: &[String]) -> Result<usize> {
    let targets_all = &self.run(dir, &["-t", "targets", "all"])?;

    let targets_all = targets_all.lines().filter_map(|l| l.rsplit_once(": "));

    let known = targets_all.clone().map(|(p, _)| p).collect::<Vec<_>>();
    let objects_known_to_ninja = objects
      .iter()
      .filter(|o| known.contains(&o.as_str()))
      .cloned()
      .collect::<Vec<_>>();
    let deps =
      self.generated_deps(dir, targets_all, &objects_known_to_ninja)?;
    self.build(dir, &deps)?;
    Ok(objects_known_to_ninja.len())
  }

  /// Runs `ninja -C <build_dir> <args>` and returns its stdout.
  /// On failure the error carries Ninja's stderr.
  fn run(&self, build_dir: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(&self.ninja);
    cmd.arg("-C").arg(build_dir);
    let out = cmd
      .args(args)
      .stdin(Stdio::null())
      .output()
      .into_diagnostic()
      .with_context(|| format!("failed to run {}", self.ninja.display()))?;
    if !out.status.success() {
      bail!(
        "{} {} failed: {}",
        self.ninja.display(),
        summarize_args(args),
        String::from_utf8_lossy(&out.stderr).trim()
      );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
  }

  /// Custom-command outputs among the transitive inputs of `objects`.
  /// `targets_all` is the output of `ninja -t targets all` for the same Ninja
  /// file.
  ///
  /// Uses `ninja -t inputs -E -0 <objects>` (needs [`supports_inputs_query`]):
  /// it lists every transitive input, including implicit and order-only ones
  /// (verified on Ninja 1.13.2 against a CMake build, where the generated
  /// header is reached only through an order-only phony), unescaped and
  /// NUL-separated. Intersecting with the `CUSTOM_COMMAND` outputs from `-t
  /// targets all` leaves the generated files. Custom targets that nothing
  /// compiles depend on are never reachable, so they are not returned. This
  /// is simpler than walking `-t query` and needs no visited set. Objects go
  /// in chunks so huge projects stay under ARG_MAX.
  fn generated_deps<'a>(
    &self,
    build_dir: &Path,
    targets_all: impl Iterator<Item = (&'a str, &'a str)>,
    objects: &[String],
  ) -> Result<Vec<String>> {
    if objects.is_empty() {
      return Ok(Vec::new());
    }
    let custom = custom_outputs(targets_all);
    let mut found = BTreeSet::new();
    for chunk in chunk_by_len(objects, MAX_ARGS_BYTES) {
      let mut args = vec!["-t", "inputs", "-E", "-0"];
      args.extend(chunk.iter().map(String::as_str));
      let inputs = self.run(build_dir, &args)?;
      found.extend(
        split_inputs(&inputs)
          .into_iter()
          .filter(|i| custom.contains(*i))
          .map(String::from),
      );
    }
    Ok(found.into_iter().collect())
  }
}

/// The paths a custom command builds, from the `(path, rule)` pairs of `ninja
/// -t targets all`.
fn custom_outputs<'a>(
  targets_all: impl Iterator<Item = (&'a str, &'a str)>,
) -> BTreeSet<String> {
  targets_all
    .filter_map(|(path, rule)| {
      let custom =
        matches!(rule.trim(), "CUSTOM_COMMAND" | "CUSTOM_COMMAND_DEP");
      (custom && !path.is_empty()).then(|| path.to_string())
    })
    .collect()
}

/// Splits `ninja -t inputs -0` output into paths.
fn split_inputs(text: &str) -> Vec<&str> {
  text.split('\0').filter(|s| !s.is_empty()).collect()
}

/// Roughly how many bytes of targets go on one Ninja command line, kept well
/// under ARG_MAX.
const MAX_ARGS_BYTES: usize = 100_000;

/// Splits `items` into consecutive chunks whose cumulative byte length (plus
/// one separator per item) is at most `max_bytes`; an oversized item gets a
/// chunk of its own.
fn chunk_by_len(items: &[String], max_bytes: usize) -> Vec<&[String]> {
  let mut chunks = Vec::new();
  let (mut start, mut size) = (0, 0);
  for (i, item) in items.iter().enumerate() {
    let cost = item.len() + 1;
    if i > start && size + cost > max_bytes {
      chunks.push(&items[start..i]);
      start = i;
      size = 0;
    }
    size += cost;
  }
  if start < items.len() {
    chunks.push(&items[start..]);
  }
  chunks
}

/// First few arguments plus a count of the rest, for error messages.
fn summarize_args(args: &[&str]) -> String {
  const SHOWN: usize = 5;
  if args.len() <= SHOWN {
    args.join(" ")
  } else {
    format!("{} ... ({} more)", args[..SHOWN].join(" "), args.len() - SHOWN)
  }
}

/// The object file of each entry as Ninja names it: relative to the build
/// directory. Uses `output`, else the argument after `-o`. `output` is resolved
/// against the entry's `directory`: Meson writes it relative to `directory`
/// (the build directory), CMake 4.x writes it absolute, and CMake 3.20 writes
/// none. Objects outside the build directory are dropped.
fn objects(db: &Db, build_dir: &Path) -> Vec<String> {
  let build_dirs = spellings(build_dir);
  let found: BTreeSet<String> = db
    .entries()
    .iter()
    .filter_map(|e| {
      let out = e.get_output()?;
      spellings(&e.directory)
        .iter()
        .find_map(|dir| relative_to_any(&dir.join(&out), &build_dirs))
    })
    .collect();
  found.into_iter().collect()
}

/// `path` as written, plus its canonical form if it exists. The database and
/// CMakeCache.txt may reach the same directory through different symlinks
/// (macOS's `/tmp` is `/private/tmp`), so paths are compared in both forms.
fn spellings(path: &Path) -> Vec<PathBuf> {
  let mut all = vec![path.to_path_buf()];
  all.extend(path.canonicalize().ok());
  all
}

/// `path` relative to the first of `roots` it is under.
fn relative_to_any(path: &Path, roots: &[PathBuf]) -> Option<String> {
  roots
    .iter()
    .find_map(|root| path.strip_prefix(root).ok()?.to_str().map(String::from))
}

#[cfg(test)]
mod tests {
  use db::Entry;

  use super::*;

  #[test]
  fn custom_outputs_keeps_only_custom_commands() {
    let targets = [
      ("gen.h", "CUSTOM_COMMAND"),
      ("x.h", "CUSTOM_COMMAND_DEP"),
      ("lib.o", "CXX_COMPILER__lib_Debug"),
      ("build.ninja", "RERUN_CMAKE"),
      ("all", "phony"),
    ];
    assert_eq!(
      custom_outputs(targets.into_iter()),
      BTreeSet::from(["gen.h".to_string(), "x.h".to_string()])
    );
  }

  fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
  }

  #[test]
  fn chunk_by_len_fills_chunks_up_to_the_limit() {
    // Each item costs its length plus one: 5, 5, 5, 3.
    let items = strings(&["aaaa", "bbbb", "cccc", "dd"]);
    assert_eq!(chunk_by_len(&items, 10), [&items[0..2], &items[2..4]]);
  }

  #[test]
  fn chunk_by_len_gives_an_oversized_item_its_own_chunk() {
    let items = strings(&["a", "bbbbbbbbbbbb", "c"]);
    assert_eq!(
      chunk_by_len(&items, 5),
      [&items[0..1], &items[1..2], &items[2..3]]
    );
  }

  #[test]
  fn chunk_by_len_of_nothing_is_no_chunks() {
    assert!(chunk_by_len(&[], 5).is_empty());
  }

  fn entry(dir: &Path, output: Option<&str>) -> Entry {
    Entry {
      directory: dir.to_path_buf(),
      file: "/src/a.cc".into(),
      arguments: strings(&["cc", "-c", "a.cc"]),
      output: output.map(PathBuf::from),
    }
  }

  fn objects_of(entries: Vec<Entry>, build_dir: &Path) -> Vec<String> {
    objects(&Db::normalize(entries).unwrap(), build_dir)
  }

  #[test]
  fn objects_are_relative_to_the_build_dir() {
    let b = Path::new("/nonexistent/build");
    let entries = vec![
      // CMake 4.x writes absolute outputs.
      entry(b, Some("/nonexistent/build/CMakeFiles/a.dir/a.cc.o")),
      // Relative outputs are relative to the entry's directory.
      entry(b, Some("CMakeFiles/b.dir/b.cc.o")),
    ];
    assert_eq!(
      objects_of(entries, b),
      ["CMakeFiles/a.dir/a.cc.o", "CMakeFiles/b.dir/b.cc.o"]
    );
  }

  #[test]
  fn objects_skip_entries_without_output_or_outside_the_build_dir() {
    let b = Path::new("/nonexistent/build");
    let entries = vec![
      entry(b, None),
      entry(Path::new("/elsewhere"), Some("e.o")),
      entry(b, Some("/elsewhere/f.o")),
    ];
    assert!(objects_of(entries, b).is_empty());
  }

  #[test]
  fn objects_are_sorted_without_duplicates() {
    let b = Path::new("/nonexistent/build");
    let mut other_flags = entry(b, Some("a.o"));
    other_flags.arguments.push("-DX".into());
    let entries =
      vec![entry(b, Some("b.o")), entry(b, Some("a.o")), other_flags];
    assert_eq!(objects_of(entries, b), ["a.o", "b.o"]);
  }

  /// A real build directory and a symlink to it, like macOS's `/tmp` and
  /// `/private/tmp`.
  fn linked_build_dir() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().canonicalize().unwrap().join("build");
    std::fs::create_dir(&real).unwrap();
    let link = t.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    (t, real, link)
  }

  #[test]
  fn objects_match_a_build_dir_reached_through_a_symlink() {
    let (_t, real, link) = linked_build_dir();
    assert_eq!(objects_of(vec![entry(&real, Some("a.o"))], &link), ["a.o"]);
  }

  #[test]
  fn objects_match_entries_reached_through_a_symlink() {
    let (_t, real, link) = linked_build_dir();
    assert_eq!(objects_of(vec![entry(&link, Some("a.o"))], &real), ["a.o"]);
  }
}
