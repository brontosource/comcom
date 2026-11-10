use std::path::PathBuf;

use miette::miette;
use miette::Report;
use miette::Result;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Entry {
  pub directory: PathBuf,
  pub file: PathBuf,
  pub arguments: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub output: Option<PathBuf>,
}

impl Entry {
  /// Returns the specified `output` field if present. Otherwise returns the
  /// argument following `-o` in `arguments` if present. Otherwise returns
  /// `None`.
  pub fn get_output(&self) -> Option<PathBuf> {
    self.output.clone().or_else(|| {
      self
        .arguments
        .iter()
        .position(|a| a == "-o")
        .and_then(|i| self.arguments.get(i + 1).map(PathBuf::from))
    })
  }
}

/// An entry as written to disk: either `arguments` or a shell `command`.
#[derive(Deserialize)]
pub(crate) struct RawEntry {
  directory: PathBuf,
  file: PathBuf,
  #[serde(default)]
  command: Option<String>,
  #[serde(default)]
  arguments: Option<Vec<String>>,
  #[serde(default)]
  output: Option<PathBuf>,
}

impl RawEntry {
  /// The entry's arguments. `arguments` wins over `command` when both exist.
  fn extract_arguments(&self) -> Result<Vec<String>> {
    match (&self.arguments, &self.command) {
      (Some(a), _) => Ok(a.clone()),
      (None, Some(c)) => shlex::split(c).ok_or_else(|| {
        miette!("Cannot split command for {}: {c}", self.file.display())
      }),
      (None, None) => Err(miette!(
        "Entry for {} has neither command nor arguments",
        self.file.display()
      )),
    }
  }
}

impl TryFrom<RawEntry> for Entry {
  type Error = Report;

  fn try_from(r: RawEntry) -> Result<Entry> {
    Ok(Entry {
      arguments: r.extract_arguments()?,
      directory: r.directory,
      file: r.file,
      output: r.output,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(arguments: &[&str], output: Option<&str>) -> Entry {
    Entry {
      arguments: arguments.iter().map(|a| a.to_string()).collect(),
      directory: PathBuf::from("/b"),
      file: PathBuf::from("a.cc"),
      output: output.map(PathBuf::from),
    }
  }

  #[test]
  fn get_output_prefers_output_field() {
    let e = entry(&["cc", "-o", "from_args.o", "-c", "a.cc"], Some("field.o"));
    assert_eq!(e.get_output(), Some(PathBuf::from("field.o")));
  }

  #[test]
  fn get_output_falls_back_to_dash_o() {
    let e = entry(&["cc", "-c", "a.cc", "-o", "x/a.o"], None);
    assert_eq!(e.get_output(), Some(PathBuf::from("x/a.o")));
  }

  #[test]
  fn get_output_trailing_dash_o_is_none() {
    let e = entry(&["cc", "-c", "a.cc", "-o"], None);
    assert_eq!(e.get_output(), None);
  }

  #[test]
  fn get_output_without_output_is_none() {
    let e = entry(&["cc", "-c", "a.cc"], None);
    assert_eq!(e.get_output(), None);
  }

  #[test]
  fn get_output_ignores_joined_dash_o() {
    // Only the separate `-o <path>` form is recognized.
    let e = entry(&["cc", "-c", "a.cc", "-oa.o"], None);
    assert_eq!(e.get_output(), None);
  }
}
