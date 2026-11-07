use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use miette::Result;

use super::parse::FromCmakeStr;

/// The values in a build directory's `CMakeCache.txt`.
pub struct Cache {
  path: PathBuf,
  content: HashMap<String, String>,
}

impl Cache {
  /// Parses `<build_dir>/CMakeCache.txt` into a key -> value map (the `:TYPE`
  /// suffix is dropped).
  pub fn read_from(build_dir: &Path) -> Result<Cache> {
    let path = build_dir.join("CMakeCache.txt");
    let text = std::fs::read_to_string(&path)
      .into_diagnostic()
      .with_context(|| format!("While reading {}", path.display()))?;
    let content = text
      .lines()
      .map(str::trim)
      .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
      .filter_map(|l| {
        let (key, value) = l.split_once('=')?;
        let key = key.split_once(':').map_or(key, |(k, _)| k);
        Some((key.to_string(), value.to_string()))
      })
      .collect();
    Ok(Cache { path, content })
  }

  /// The value of `key`, parsed as an `R`, if the cache has one.
  pub fn get<R: FromCmakeStr>(&self, key: &str) -> Option<Result<R>> {
    self.content.get(key).map(|s| {
      R::from_cmake_str(s)
        .with_context(|| format!("{key} in {}", self.path.display()))
    })
  }

  /// Like [`Cache::get`], but a missing `key` is an error.
  pub fn require<R: FromCmakeStr>(&self, key: &str) -> Result<R> {
    self
      .get(key)
      .unwrap_or_else(|| Err(miette!("{} has no {key}", self.path.display())))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cache_parsing() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
      t.path().join("CMakeCache.txt"),
      "# c\n// d\n\nCMAKE_GENERATOR:INTERNAL=Ninja\nX:BOOL=ON\nN:STRING=3\n",
    )
    .unwrap();
    let c = Cache::read_from(t.path()).unwrap();
    assert_eq!(c.content.len(), 3);
    assert_eq!(c.require::<String>("CMAKE_GENERATOR").unwrap(), "Ninja");
    assert_eq!(c.require::<String>("X").unwrap(), "ON");
    assert!(c.require::<bool>("X").unwrap());
    assert_eq!(c.require::<String>("N").unwrap(), "3");
    assert!(c.get::<String>("missing").is_none());
    assert!(c.require::<String>("missing").is_err());
    assert!(c.require::<bool>("CMAKE_GENERATOR").is_err());
  }
}
