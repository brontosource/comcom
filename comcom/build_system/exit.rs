//! Describing how a build tool's process ended.

use std::process::ExitStatus;

/// How a process that ended with `status` ended, for an error message:
/// "Exit 2", or "Terminated by signal 9".
pub(crate) fn describe(status: ExitStatus) -> String {
  if let Some(n) = status.code() {
    return format!("Exit {n}");
  }
  #[cfg(unix)]
  {
    use std::os::unix::process::ExitStatusExt;
    if let Some(signal) = status.signal() {
      return format!("Terminated by signal {signal}");
    }
  }
  "Terminated by signal".to_string()
}

#[cfg(all(test, unix))]
mod tests {
  use std::os::unix::process::ExitStatusExt;

  use super::*;

  #[test]
  fn exit_code() {
    assert_eq!(describe(ExitStatus::from_raw(3 << 8)), "Exit 3");
  }

  #[test]
  fn signal() {
    assert_eq!(describe(ExitStatus::from_raw(9)), "Terminated by signal 9");
  }
}
