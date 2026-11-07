//! Building a [`Db`] from entries: the only way to make one.

use std::path::Path;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;

use crate::Db;
use crate::Entry;

impl Db {
  /// Expands `@file` response files (relative to each entry's directory),
  /// then sorts and removes exact duplicates (see [`Db::merge`]).
  pub fn normalize(entries: impl IntoIterator<Item = Entry>) -> Result<Db> {
    Db::normalize_with(entries, |_| Ok(()))
  }

  /// Like [`Db::normalize`], but runs `rewrite` on each entry after its
  /// response files are expanded, for changes specific to a build system.
  pub fn normalize_with(
    entries: impl IntoIterator<Item = Entry>,
    mut rewrite: impl FnMut(&mut Entry) -> Result<()>,
  ) -> Result<Db> {
    let entries = entries
      .into_iter()
      .map(|mut e| {
        e.arguments =
          expand_params(std::mem::take(&mut e.arguments), &e.directory)?;
        rewrite(&mut e)?;
        Ok(e)
      })
      .collect::<Result<Vec<_>>>()?;
    Ok(Db::merge([Db(entries)]))
  }
}

/// Replaces each `@file` argument whose file exists (relative to `base`) with
/// the arguments in that file. Other arguments are kept as is.
fn expand_params(args: Vec<String>, base: &Path) -> Result<Vec<String>> {
  let mut out = Vec::with_capacity(args.len());
  for arg in args {
    if let Some(rel) = arg.strip_prefix('@') {
      let path = base.join(rel);
      if path.is_file() {
        let contents = std::fs::read_to_string(&path)
          .into_diagnostic()
          .with_context(|| {
            format!("Reading parameter file {}", path.display())
          })?;
        let parts = shlex::split(&contents).ok_or_else(|| {
          miette!("Cannot parse parameter file {}", path.display())
        })?;
        out.extend(parts);
        continue;
      }
    }
    out.push(arg);
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(dir: &str, args: &[&str]) -> Entry {
    Entry {
      directory: dir.into(),
      file: "a.cc".into(),
      arguments: args.iter().map(|s| s.to_string()).collect(),
      output: Some("a.o".into()),
    }
  }

  fn norm(args: &[&str]) -> Vec<String> {
    Db::normalize([entry("/nonexistent", args)])
      .unwrap()
      .into_entries()
      .remove(0)
      .arguments
  }

  #[test]
  fn rsp_expanded_relative_to_directory() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("CMakeFiles")).unwrap();
    std::fs::write(d.path().join("CMakeFiles/x.rsp"), "-Iinc -DX=1").unwrap();
    let db = Db::normalize([entry(
      d.path().to_str().unwrap(),
      &["cc", "@CMakeFiles/x.rsp", "-c", "a.cc"],
    )])
    .unwrap();
    assert_eq!(
      db.entries()[0].arguments,
      ["cc", "-Iinc", "-DX=1", "-c", "a.cc"]
    );
  }

  #[test]
  fn missing_rsp_kept() {
    assert_eq!(norm(&["cc", "@missing.rsp"]), ["cc", "@missing.rsp"]);
  }

  #[test]
  fn duplicates_removed() {
    let e = entry("/b", &["cc", "-c", "a.cc"]);
    let db = Db::normalize([e.clone(), e]).unwrap();
    assert_eq!(db.len(), 1);
  }

  #[test]
  fn rewrite_sees_expanded_arguments() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("x.rsp"), "-DX=1").unwrap();
    let mut seen = Vec::new();
    Db::normalize_with(
      [entry(d.path().to_str().unwrap(), &["cc", "@x.rsp"])],
      |e| {
        seen = e.arguments.clone();
        Ok(())
      },
    )
    .unwrap();
    assert_eq!(seen, ["cc", "-DX=1"]);
  }

  #[test]
  fn rewrite_runs_before_dedup() {
    let db = Db::normalize_with(
      [
        entry("/b", &["cc", "-DA", "-c", "a.cc"]),
        entry("/b", &["cc", "-DB", "-c", "a.cc"]),
      ],
      |e| {
        e.arguments.retain(|a| !a.starts_with("-D"));
        Ok(())
      },
    )
    .unwrap();
    assert_eq!(db.len(), 1);
  }

  #[test]
  fn rewrite_error_fails_normalize() {
    let result =
      Db::normalize_with([entry("/b", &["cc"])], |_| Err(miette!("nope")));
    assert!(result.is_err());
  }
}
