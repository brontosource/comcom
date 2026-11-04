use miette::IntoDiagnostic;
use miette::Result;
use serde::Deserialize;
use serde::Serialize;

pub const EXEC_ROOT_PLACEHOLDER: &str = "__EXEC_ROOT__";

mod entry;
pub use entry::Entry;

/// The entries of one compile_commands.json file or fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Db(Vec<Entry>);

impl Db {
  pub fn new() -> Db {
    Db::default()
  }

  pub fn from_json_str(json: &str) -> miette::Result<Db> {
    serde_json::from_str(json).into_diagnostic()
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
  use std::path::PathBuf;

  use super::*;

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
}
