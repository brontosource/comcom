//! Runs a build under `ptrace` and records the compilations. Linux only: on
//! macOS, System Integrity Protection makes this approach infeasible.

#[cfg(any(target_os = "linux", test))]
pub(crate) mod classify;
#[cfg(target_os = "linux")]
mod pid;
#[cfg(target_os = "linux")]
pub(crate) mod trace;
#[cfg(any(target_os = "linux", test))]
pub(crate) mod warnings;

/// Runs `cmd` to completion under `ptrace` and returns the compilations it ran,
/// along with warnings about any that may be missing.
#[cfg(target_os = "linux")]
pub(crate) fn snapshot(
  cmd: std::process::Command,
  tool_name: &str,
) -> miette::Result<(db::Db, warnings::Warnings)> {
  let traced = trace::run(cmd)?;
  if !traced.status.success() {
    miette::bail!(
      "{tool_name} failed ({})",
      crate::exit::describe(traced.status)
    );
  }

  let mut warnings = traced.problems;
  for argv in &traced.still_running {
    warnings.push(format!(
      "`{}` was still running when the build finished, so anything it \
       compiles later is not recorded",
      argv.join(" ")
    ));
  }
  let analysis = classify::analyze(&traced.execs);
  warnings.extend(analysis.warnings);
  Ok((db::Db::normalize(analysis.entries)?, warnings))
}
