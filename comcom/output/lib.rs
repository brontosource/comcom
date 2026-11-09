use std::io::Write;
use std::path::Path;

use miette::bail;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;
use miette::Severity;

/// Prints `message` to stderr as a miette report of the given `severity`.
fn report(severity: Severity, message: &str) {
  eprintln!("{:?}", miette!(severity = severity, "{message}"));
}

pub fn advise(message: &str) {
  report(Severity::Advice, message)
}

pub fn warn(message: &str) {
  report(Severity::Warning, message)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteOutcome {
  Written,
  Unchanged,
}

/// Returns true if `path` exists and its bytes equal `contents`.
pub fn is_up_to_date(path: &Path, contents: &str) -> Result<bool> {
  match std::fs::read(path) {
    Ok(existing) => Ok(existing == contents.as_bytes()),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
    Err(e) => Err(e)
      .into_diagnostic()
      .with_context(|| format!("Failed to read {}", path.display())),
  }
}

/// Atomically writes `contents` to `path`, leaving the file untouched if the
/// contents already match. Specifically, if `path` is a symlink and the
/// contents match, the symlink is kept intact. If the contents changed, the
/// symlink is replaced with a regular file.
pub fn write_if_changed(path: &Path, contents: &str) -> Result<WriteOutcome> {
  let parent = match path.parent() {
    Some(p) if !p.as_os_str().is_empty() => p,
    _ => Path::new("."),
  };
  if !parent.is_dir() {
    bail!("Output directory {} does not exist", parent.display());
  }
  if is_up_to_date(path, contents)? {
    return Ok(WriteOutcome::Unchanged);
  }
  let mut tmp = new_temp_file(path).into_diagnostic().with_context(|| {
    format!("Failed to create temp file in {}", parent.display())
  })?;
  // Keep the mode of the regular file being replaced. A symlink is replaced
  // by a new file, so it gets the default mode rather than the target's.
  if let Some(meta) = std::fs::symlink_metadata(path)
    .ok()
    .filter(|m| m.file_type().is_file())
  {
    tmp
      .as_file()
      .set_permissions(meta.permissions())
      .into_diagnostic()
      .with_context(|| {
        format!("Failed to set permissions for {}", path.display())
      })?;
  }
  tmp
    .write_all(contents.as_bytes())
    .into_diagnostic()
    .with_context(|| {
      format!("Failed to write temp file for {}", path.display())
    })?;
  tmp
    .persist(path)
    .map_err(|e| e.error)
    .into_diagnostic()
    .with_context(|| format!("Failed to replace {}", path.display()))?;
  Ok(WriteOutcome::Written)
}

/// A temp file next to `path`, to be renamed over it. It is named
/// `.<file name>.<random>` so a leftover from an interrupted write shows where
/// it came from. It has the mode a newly created file would get: on Unix, 0666
/// minus the umask (applied by the kernel), not tempfile's default 0600.
fn new_temp_file(path: &Path) -> std::io::Result<tempfile::NamedTempFile> {
  let dir = match path.parent() {
    Some(p) if !p.as_os_str().is_empty() => p,
    _ => Path::new("."),
  };
  let mut prefix = std::ffi::OsString::from(".");
  if let Some(name) = path.file_name() {
    prefix.push(name);
    prefix.push(".");
  }
  let mut builder = tempfile::Builder::new();
  builder.prefix(&prefix);
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    builder.permissions(std::fs::Permissions::from_mode(0o666));
  }
  builder.tempfile_in(dir)
}

#[cfg(test)]
mod tests {
  use std::time::Duration;
  use std::time::UNIX_EPOCH;

  use super::*;

  #[test]
  fn writes_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    assert_eq!(write_if_changed(&p, "hello").unwrap(), WriteOutcome::Written);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
  }

  #[test]
  fn unchanged_preserves_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    write_if_changed(&p, "x").unwrap();
    let t = UNIX_EPOCH + Duration::from_secs(1000);
    std::fs::File::options()
      .write(true)
      .open(&p)
      .unwrap()
      .set_modified(t)
      .unwrap();
    assert_eq!(write_if_changed(&p, "x").unwrap(), WriteOutcome::Unchanged);
    assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), t);
  }

  #[test]
  fn rewrites_when_different() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    write_if_changed(&p, "x").unwrap();
    assert_eq!(write_if_changed(&p, "y").unwrap(), WriteOutcome::Written);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "y");
  }

  #[cfg(unix)]
  fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
  }

  #[cfg(unix)]
  #[test]
  fn new_file_gets_default_create_mode() {
    let dir = tempfile::tempdir().unwrap();
    // A plain create gets 0666 minus the umask: the expected mode.
    let reference = dir.path().join("reference");
    std::fs::File::create(&reference).unwrap();
    let p = dir.path().join("out.json");
    write_if_changed(&p, "x").unwrap();
    assert_eq!(mode(&p), mode(&reference));
    assert_ne!(mode(&p), 0o600);
  }

  #[cfg(unix)]
  #[test]
  fn rewrite_preserves_existing_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    std::fs::write(&p, "x").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640))
      .unwrap();
    assert_eq!(write_if_changed(&p, "y").unwrap(), WriteOutcome::Written);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "y");
    assert_eq!(mode(&p), 0o640);
  }

  #[test]
  fn missing_parent_dir_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nope").join("out.json");
    let err = write_if_changed(&p, "x").unwrap_err();
    assert!(err.to_string().contains("does not exist"), "{err}");
    assert!(!dir.path().join("nope").exists());
  }

  #[test]
  fn up_to_date_false_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_up_to_date(&dir.path().join("out.json"), "x").unwrap());
  }

  #[test]
  fn up_to_date_true_when_identical() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    std::fs::write(&p, "x").unwrap();
    assert!(is_up_to_date(&p, "x").unwrap());
    assert!(!is_up_to_date(&p, "y").unwrap());
  }

  #[cfg(unix)]
  fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
      .unwrap()
      .file_type()
      .is_symlink()
  }

  #[cfg(unix)]
  #[test]
  fn up_to_date_follows_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.json");
    std::fs::write(&target, "x").unwrap();
    let p = dir.path().join("out.json");
    std::os::unix::fs::symlink(&target, &p).unwrap();
    assert!(is_up_to_date(&p, "x").unwrap());
    assert!(!is_up_to_date(&p, "y").unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn up_to_date_false_for_dangling_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.json");
    std::os::unix::fs::symlink(dir.path().join("missing"), &p).unwrap();
    assert!(!is_up_to_date(&p, "x").unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn keeps_symlink_to_identical_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.json");
    std::fs::write(&target, "x").unwrap();
    let p = dir.path().join("out.json");
    std::os::unix::fs::symlink(&target, &p).unwrap();

    assert_eq!(write_if_changed(&p, "x").unwrap(), WriteOutcome::Unchanged);
    assert!(is_symlink(&p));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
  }

  #[cfg(unix)]
  #[test]
  fn replaces_dangling_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let p = dir.path().join("out.json");
    std::os::unix::fs::symlink(&missing, &p).unwrap();

    assert_eq!(write_if_changed(&p, "x").unwrap(), WriteOutcome::Written);
    assert!(!is_symlink(&p));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "x");
    assert!(!missing.exists());
  }

  #[cfg(unix)]
  #[test]
  fn replaces_symlink_with_different_contents() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.json");
    std::fs::write(&target, "x").unwrap();
    let p = dir.path().join("out.json");
    std::os::unix::fs::symlink(&target, &p).unwrap();

    assert_eq!(write_if_changed(&p, "y").unwrap(), WriteOutcome::Written);
    let meta = std::fs::symlink_metadata(&p).unwrap();
    assert!(meta.file_type().is_file(), "{p:?} is still a symlink");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "y");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
  }

  #[test]
  fn temp_file_named_after_output() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = new_temp_file(&dir.path().join("out.json")).unwrap();
    assert_eq!(tmp.path().parent(), Some(dir.path()));
    let name = tmp.path().file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(".out.json."), "{name}");
  }
}
