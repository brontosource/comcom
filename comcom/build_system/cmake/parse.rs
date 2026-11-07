use std::path::PathBuf;

use miette::miette;
use miette::Result;

/// Parsing a value as CMake reads it, e.g. a value in `CMakeCache.txt`.
pub trait FromCmakeStr: Sized {
  fn from_cmake_str(s: &str) -> Result<Self>;
}

/// The truth value CMake's `if()` gives a constant: `ON`, `YES`, `TRUE`, `Y`
/// and non-zero numbers are true; `OFF`, `NO`, `FALSE`, `N`, `IGNORE`,
/// `NOTFOUND`, `0`, the empty string and anything ending in `-NOTFOUND` are
/// false. Anything else is an error.
impl FromCmakeStr for bool {
  fn from_cmake_str(s: &str) -> Result<bool> {
    let upper = s.to_ascii_uppercase();
    if matches!(
      upper.as_str(),
      "" | "0" | "FALSE" | "OFF" | "NO" | "N" | "IGNORE" | "NOTFOUND"
    ) || upper.ends_with("-NOTFOUND")
    {
      Ok(false)
    } else if matches!(upper.as_str(), "TRUE" | "ON" | "YES" | "Y") {
      Ok(true)
    } else if let Ok(n) = s.parse::<f64>() {
      Ok(n != 0.0)
    } else {
      Err(miette!("Unable to parse {s:?} as a boolean value."))
    }
  }
}

impl FromCmakeStr for String {
  fn from_cmake_str(s: &str) -> Result<String> {
    Ok(s.to_string())
  }
}

impl FromCmakeStr for PathBuf {
  fn from_cmake_str(s: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(s))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse_bool(s: &str) -> Option<bool> {
    bool::from_cmake_str(s).ok()
  }

  #[test]
  fn cmake_false_values() {
    for v in [
      "OFF",
      "off",
      "0",
      "FALSE",
      "No",
      "n",
      "IGNORE",
      "NOTFOUND",
      "",
      "X-NOTFOUND",
      "x-notfound",
    ] {
      assert_eq!(parse_bool(v), Some(false), "{v:?}");
    }
  }

  #[test]
  fn cmake_true_values() {
    for v in ["ON", "on", "1", "2", "TRUE", "YES", "Y", "y"] {
      assert_eq!(parse_bool(v), Some(true), "{v:?}");
    }
  }

  #[test]
  fn other_values_are_errors() {
    assert_eq!(parse_bool("anything"), None);
  }
}
