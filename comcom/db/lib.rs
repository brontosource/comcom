use std::path::Path;

use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;
use serde::Deserialize;
use serde::Serialize;

mod entry;
pub use entry::Entry;
use entry::RawEntry;

/// The entries of one compile_commands.json file or fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Db(Vec<Entry>);

impl Db {
  pub fn new() -> Db {
    Db::default()
  }

  /// Reads the compile_commands.json file at `path`.
  pub fn read_from(path: &Path) -> Result<Db> {
    let text = std::fs::read_to_string(path)
      .into_diagnostic()
      .with_context(|| format!("While reading {}", path.display()))?;
    Self::from_json_str(&text)
      .with_context(|| format!("While parsing {}", path.display()))
  }

  pub fn from_json_str(json: &str) -> miette::Result<Db> {
    let raw: Vec<RawEntry> = serde_json::from_str(json).into_diagnostic()?;
    Ok(Db(
      raw
        .into_iter()
        .map(Entry::try_from)
        .collect::<Result<Vec<_>>>()?,
    ))
  }

  pub fn entries(&self) -> &[Entry] {
    &self.0
  }

  pub fn into_entries(self) -> Vec<Entry> {
    self.0
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn render(&self) -> Result<String> {
    let mut s =
      serde_json::to_string_pretty(self.entries()).into_diagnostic()?;
    s.push('\n');
    Ok(s)
  }

  /// Concatenates fragments, sorts deterministically and drops exact
  /// duplicates.
  pub fn merge(fragments: impl IntoIterator<Item = Db>) -> Db {
    let mut all: Vec<Entry> =
      fragments.into_iter().flat_map(Db::into_entries).collect();
    all.sort_by(|a, b| {
      (&a.file, &a.output, &a.arguments, &a.directory).cmp(&(
        &b.file,
        &b.output,
        &b.arguments,
        &b.directory,
      ))
    });
    all.dedup();
    Db(all)
  }
}

impl<I: IntoIterator<Item = Entry>> From<I> for Db {
  fn from(entries: I) -> Db {
    Db(entries.into_iter().collect())
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;
  use std::path::PathBuf;

  use super::*;

  const EXEC_ROOT_PLACEHOLDER: &str = "__EXEC_ROOT__";

  fn entry(file: &str, out: &str) -> Entry {
    Entry {
      directory: EXEC_ROOT_PLACEHOLDER.into(),
      file: file.into(),
      arguments: vec!["cc".into(), "-c".into(), file.into()],
      output: Some(out.into()),
    }
  }

  fn parse(json: &str) -> miette::Result<Vec<Entry>> {
    Db::from_json_str(json).map(Db::into_entries)
  }

  fn db(entries: &[Entry]) -> Db {
    entries.to_vec().into()
  }
  #[test]
  fn parse_reads_aspect_fragment() {
    let v = parse(r#"[{"directory":"__EXEC_ROOT__","file":"a.cc","arguments":["cc","-c","a.cc"],"output":"a.o"}]"#).unwrap();
    assert_eq!(v, vec![entry("a.cc", "a.o")]);
  }
  #[test]
  fn parse_accepts_missing_output() {
    let v =
      parse(r#"[{"directory":"d","file":"a.cc","arguments":["cc"]}]"#).unwrap();
    assert_eq!(v[0].output, None);
  }
  #[test]
  fn parse_rejects_malformed() {
    assert!(parse(r#"[{"file":1}]"#).is_err());
  }
  #[test]
  fn merge_drops_exact_duplicates() {
    assert_eq!(
      Db::merge([db(&[entry("a.cc", "a.o")]), db(&[entry("a.cc", "a.o")])])
        .len(),
      1
    );
  }
  #[test]
  fn merge_keeps_same_file_in_two_configs() {
    let m = Db::merge([db(&[
      entry("a.cc", "bazel-out/k8-fastbuild/bin/a.o"),
      entry("a.cc", "bazel-out/k8-opt/bin/a.o"),
    ])]);
    assert_eq!(m.len(), 2);
  }
  #[test]
  fn merge_is_order_independent() {
    let (x, y) = (entry("b.cc", "b.o"), entry("a.cc", "a.o"));
    let m = Db::merge([db(&[x.clone()]), db(&[y.clone()])]);
    assert_eq!(m, Db::merge([db(&[y]), db(&[x])]));
    assert_eq!(m.entries()[0].file, PathBuf::from("a.cc"));
  }
  #[test]
  fn render_empty_is_brackets() {
    assert_eq!(Db::new().render().unwrap(), "[]\n");
  }
  #[test]
  fn render_round_trips() {
    let mut x = entry("a.cc", "a.o");
    x.arguments.push(r#"-DGREETING="hello world""#.into());
    let s = Db::from([x.clone()]).render().unwrap();
    assert!(s.ends_with('\n'));
    assert_eq!(parse(&s).unwrap(), vec![x]);
  }

  #[test]
  fn command_is_split() {
    let e = parse(r#"[{"directory":"/b","file":"a.cc","output":"a.o",
            "command":"/usr/bin/c++ '-DG=\"hello world\"' -I\"/p q/inc\" -c a.cc"}]"#).unwrap();
    assert_eq!(
      e[0].arguments,
      ["/usr/bin/c++", "-DG=\"hello world\"", "-I/p q/inc", "-c", "a.cc"]
    );
  }
  #[test]
  fn arguments_form_kept() {
    let e = parse(
      r#"[{"directory":"/b","file":"a.cc","arguments":["cc","-c","a.cc"]}]"#,
    )
    .unwrap();
    assert_eq!(e[0].arguments, ["cc", "-c", "a.cc"]);
  }
  #[test]
  fn arguments_win_over_command() {
    let e = parse(r#"[{"directory":"/b","file":"a.cc","arguments":["cc"],"command":"other -c a.cc"}]"#).unwrap();
    assert_eq!(e[0].arguments, ["cc"]);
  }
  #[test]
  fn missing_output_stays_missing() {
    let e = parse(r#"[{"directory":"/b","file":"a.cc","arguments":["cc"]}]"#)
      .unwrap();
    assert_eq!(e[0].output, None);
  }
  #[test]
  fn output_kept_exactly() {
    let e = parse(r#"[{"directory":"/b","file":"a.cc","output":"x/../a.o","arguments":["cc"]}]"#).unwrap();
    assert_eq!(e[0].output.as_deref(), Some(Path::new("x/../a.o")));
  }
  #[test]
  fn neither_command_nor_arguments_errors() {
    assert!(parse(r#"[{"directory":"/b","file":"a.cc"}]"#).is_err());
  }
  #[test]
  fn unsplittable_command_errors() {
    assert!(parse(
      r#"[{"directory":"/b","file":"a.cc","command":"cc \"oops"}]"#
    )
    .is_err());
  }
}
