use std::str::FromStr;

use miette::miette;
use miette::IntoDiagnostic;
use miette::Report;
use miette::Result;

/// A `major.minor.patch` version, such as `ninja --version` prints.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemVer {
  pub major: usize,
  pub minor: usize,
  pub patch: usize,
}

impl SemVer {
  pub fn new(major: usize, minor: usize, patch: usize) -> SemVer {
    SemVer { major, minor, patch }
  }
}

impl FromStr for SemVer {
  type Err = Report;

  /// Parses strings such as `1.13.2` or `1.10.2.git`. A missing patch
  /// component is 0; anything after the patch number's digits is ignored.
  fn from_str(s: &str) -> Result<SemVer> {
    let mut parts = s.trim().split('.');
    let mut next = || {
      parts
        .next()
        .ok_or_else(|| miette!("{s:?} is not a version number"))
    };
    let major = next()?.parse().into_diagnostic()?;
    let minor = next()?.parse().into_diagnostic()?;
    let patch = parts
      .next()
      .map(|p| {
        let digits: String =
          p.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().into_diagnostic()
      })
      .unwrap_or(Ok(0))?;
    Ok(SemVer::new(major, minor, patch))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse(s: &str) -> Option<SemVer> {
    s.parse().ok()
  }

  #[test]
  fn parses_major_minor_patch() {
    assert_eq!(parse("1.13.2"), Some(SemVer::new(1, 13, 2)));
  }

  #[test]
  fn ignores_surrounding_whitespace() {
    // `ninja --version` ends with a newline.
    assert_eq!(parse(" 1.13.2\n"), Some(SemVer::new(1, 13, 2)));
  }

  #[test]
  fn missing_patch_is_zero() {
    assert_eq!(parse("1.11"), Some(SemVer::new(1, 11, 0)));
  }

  #[test]
  fn ignores_anything_after_the_patch_digits() {
    assert_eq!(parse("1.10.2.git"), Some(SemVer::new(1, 10, 2)));
    assert_eq!(parse("1.12.0-rc1"), Some(SemVer::new(1, 12, 0)));
  }

  #[test]
  fn rejects_non_versions() {
    for s in ["", "garbage", "1", "1.", "1.x.0", "-1.0.0", "1.13.x"] {
      assert_eq!(parse(s), None, "{s:?}");
    }
  }

  #[test]
  fn orders_by_major_then_minor_then_patch() {
    assert!(SemVer::new(1, 13, 0) < SemVer::new(1, 13, 2));
    assert!(SemVer::new(1, 10, 9) < SemVer::new(1, 13, 0));
    assert!(SemVer::new(1, 99, 99) < SemVer::new(2, 0, 0));
  }
}
