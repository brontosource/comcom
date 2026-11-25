//! Runs a command under `ptrace` and records every program it, and anything it
//! starts, runs. Linux only.

use std::collections::HashMap;
use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

use libc::c_int;
use miette::bail;
use miette::miette;
use miette::Result;

use super::classify::Exec;
use super::pid;
use super::pid::Pid;
use super::warnings::Warnings;

/// What a traced command ran.
#[derive(Debug)]
pub struct Traced {
  pub status: ExitStatus,
  /// Every program started, in order, the command itself first.
  pub execs: Vec<Exec>,
  /// The command lines of programs still running when the command exited.
  /// They are no longer traced.
  pub still_running: Vec<Vec<String>>,
  /// Problems that mean some program may be missing from `execs`.
  pub problems: Warnings,
}

/// Runs `cmd` to completion, tracing it and everything it starts.
pub fn run(mut cmd: Command) -> Result<Traced> {
  // SAFETY: `trace_me` is async-signal-safe and touches no memory of the
  // forked child.
  unsafe {
    cmd.pre_exec(pid::trace_me);
  }
  let program = PathBuf::from(cmd.get_program());
  let child = cmd.spawn().map_err(|e| {
    if e.raw_os_error() == Some(libc::EPERM) {
      miette!(
        help = "Tracing may be turned off: see \
                /proc/sys/kernel/yama/ptrace_scope (0 or 1 allows it), or \
                the container's seccomp profile",
        "Cannot trace {}: {e}",
        program.display()
      )
    } else {
      miette!("Failed to run {}: {e}", program.display())
    }
  })?;
  Tracer::new(Pid::from(&child)).run()
}

/// The options set on every traced process: follow every new process and
/// thread, and stop at each program start.
const OPTIONS: c_int = libc::PTRACE_O_TRACEFORK
  | libc::PTRACE_O_TRACEVFORK
  | libc::PTRACE_O_TRACECLONE
  | libc::PTRACE_O_TRACEEXEC;

struct Tracer {
  root: Pid,
  execs: Vec<Exec>,
  /// The program each traced process (or thread) is running, as an index
  /// into `execs`. `None` until the root's first program is recorded.
  current: HashMap<Pid, Option<usize>>,
  /// Processes whose first stop (a `SIGSTOP` when tracing begins) was seen.
  started: HashSet<Pid>,
  root_status: Option<c_int>,
  problems: Warnings,
  /// The programs of the processes let go after the root exited.
  let_go: HashSet<Vec<String>>,
}

impl Tracer {
  fn new(root: Pid) -> Tracer {
    Tracer {
      root,
      execs: Vec::new(),
      current: HashMap::new(),
      started: HashSet::new(),
      root_status: None,
      problems: Warnings::new(),
      let_go: HashSet::new(),
    }
  }

  fn run(mut self) -> Result<Traced> {
    // The child stops with SIGTRAP once its program has started.
    let status = self.root.wait().map_err(wait_error)?;
    if !libc::WIFSTOPPED(status) || libc::WSTOPSIG(status) != libc::SIGTRAP {
      bail!("The traced program did not start as expected ({status:#x})");
    }
    self.current.insert(self.root, None);
    self.started.insert(self.root);
    self.record_exec(self.root);
    if let Err(e) = self.root.set_options(OPTIONS) {
      bail!("Cannot trace: {e}");
    }
    self.root.resume();

    while self.root_status.is_none() {
      let Some((pid, status)) = pid::wait_any().map_err(wait_error)? else {
        break;
      };
      self.handle(pid, status, None);
    }
    let still_running = self.let_go();
    let Some(status) = self.root_status else {
      return Err(miette!("Lost track of the traced program before it exited"));
    };
    Ok(Traced {
      status: ExitStatus::from_raw(status),
      execs: self.execs,
      still_running,
      problems: self.problems,
    })
  }

  /// Handles a change of state `status` of traced `pid`. While letting go,
  /// processes in `stopping` are let go at their next `SIGSTOP`, and new
  /// processes join it.
  fn handle(
    &mut self,
    pid: Pid,
    status: c_int,
    mut stopping: Option<&mut HashSet<Pid>>,
  ) {
    if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
      self.current.remove(&pid);
      self.started.remove(&pid);
      if let Some(s) = stopping {
        s.remove(&pid);
      }
      if pid == self.root {
        self.root_status = Some(status);
      }
      return;
    }
    if !libc::WIFSTOPPED(status) {
      return;
    }
    if !self.current.contains_key(&pid) {
      self.adopt(pid);
    }
    let signal = libc::WSTOPSIG(status);
    match status >> 16 {
      libc::PTRACE_EVENT_FORK
      | libc::PTRACE_EVENT_VFORK
      | libc::PTRACE_EVENT_CLONE => {
        // Without the new id (the parent was killed), the new process is
        // adopted when it is first seen.
        if let Some(new) = pid.event_pid() {
          let program = self.current[&pid];
          // The new process may have been seen (and adopted) first.
          self.current.entry(new).or_insert(program);
          if let Some(s) = stopping.as_deref_mut() {
            s.insert(new);
          }
        }
        pid.resume();
      }
      libc::PTRACE_EVENT_EXEC => {
        // A thread other than the main one that starts a program takes over
        // the main thread's id.
        if let Some(former) = pid.event_pid().filter(|&former| former != pid) {
          if let Some(program) = self.current.remove(&former) {
            self.current.insert(pid, program);
          }
          self.started.remove(&former);
        }
        self.record_exec(pid);
        pid.resume();
      }
      0 if signal == libc::SIGSTOP
        && stopping.as_deref().is_some_and(|s| s.contains(&pid)) =>
      {
        self.started.insert(pid);
        if let Some(i) = self.current[&pid] {
          self.let_go.insert(self.execs[i].argv.clone());
        }
        pid.detach();
        if let Some(s) = stopping {
          s.remove(&pid);
        }
        self.current.remove(&pid);
      }
      0 if signal == libc::SIGSTOP && self.started.insert(pid) => {
        // A new process's first stop: not a signal anyone sent.
        pid.resume();
      }
      0 if pid.is_group_stop(signal) => {
        // A process stopping itself (e.g. on SIGTSTP). Under tracing it would
        // stay stopped, and the build with it, so it is resumed.
        pid.resume();
      }
      0 => pid.resume_with_signal(signal),
      _ => pid.resume(),
    }
  }

  /// Starts tracking `pid`, which was seen before the event that created it.
  /// It runs the program of its process's main thread, or else of its parent.
  fn adopt(&mut self, pid: Pid) {
    let owner = match pid.tgid() {
      Some(tgid) if tgid != pid => Some(tgid),
      _ => pid.ppid(),
    };
    let program = owner.and_then(|o| self.current.get(&o).copied()).flatten();
    if program.is_none() {
      self
        .problems
        .push(format!("Could not tell which program started process {pid}"));
    }
    self.current.insert(pid, program);
  }

  /// Records the program `pid` just started.
  fn record_exec(&mut self, pid: Pid) {
    let argv = match pid.cmdline() {
      Ok(argv) => argv,
      Err(e) => {
        self
          .problems
          .push(format!("Could not read the command of process {pid}: {e}"));
        return;
      }
    };
    self.execs.push(Exec {
      parent: self.current.get(&pid).copied().flatten(),
      argv,
      cwd: pid.cwd(),
      exe: pid.exe(),
    });
    self.current.insert(pid, Some(self.execs.len() - 1));
  }

  /// Stops tracing the processes left after the root exited, and returns
  /// the command lines they were running when let go. Each is let go at its
  /// next SIGSTOP, which detaching drops.
  ///
  /// A process whose first stop (from the start of tracing) has not been
  /// seen is let go at that stop. It must not be sent another SIGSTOP: if it
  /// has already taken the first, ours would arrive after it is let go and
  /// leave it stopped for good.
  fn let_go(&mut self) -> Vec<Vec<String>> {
    let mut stopping: HashSet<Pid> = self.current.keys().copied().collect();
    for pid in stopping.iter().filter(|pid| self.started.contains(*pid)) {
      pid.stop();
    }
    while !stopping.is_empty() {
      match pid::wait_any() {
        Ok(Some((pid, status))) => {
          self.handle(pid, status, Some(&mut stopping))
        }
        _ => break,
      }
    }
    let mut still_running: Vec<_> = self.let_go.drain().collect();
    still_running.sort();
    still_running
  }
}

fn wait_error(e: std::io::Error) -> miette::Report {
  miette!("Failed waiting for the traced program: {e}")
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;
  use std::time::Duration;
  use std::time::Instant;

  use super::*;

  fn sh(script: &str) -> Traced {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script);
    run(cmd).unwrap()
  }

  fn argv(t: &Traced) -> Vec<Vec<&str>> {
    t.execs
      .iter()
      .map(|e| e.argv.iter().map(String::as_str).collect())
      .collect()
  }

  fn find<'a>(t: &'a Traced, arg0: &str) -> &'a Exec {
    t.execs
      .iter()
      .find(|e| e.argv[0] == arg0)
      .unwrap_or_else(|| panic!("no {arg0} in {:?}", argv(t)))
  }

  #[test]
  fn records_the_command_itself_first() {
    let t = sh("true");
    assert_eq!(argv(&t)[0], ["/bin/sh", "-c", "true"]);
    assert_eq!(t.execs[0].parent, None);
    assert!(t.status.success());
    assert!(t.problems.is_empty(), "{:?}", t.problems);
  }

  #[test]
  fn records_programs_with_their_parents_and_directories() {
    let t = sh("cd / && /bin/true x; /bin/sh -c '/bin/echo hi >/dev/null'");
    let tru = find(&t, "/bin/true");
    assert_eq!(tru.argv, ["/bin/true", "x"]);
    assert_eq!(tru.cwd, PathBuf::from("/"));
    assert_eq!(tru.parent, Some(0));
    // `exe` has symlinks resolved; `/bin/true` may be one (e.g. to `gnutrue`).
    assert_eq!(tru.exe, std::fs::canonicalize("/bin/true").unwrap());
    let echo = find(&t, "/bin/echo");
    let inner = echo.parent.unwrap();
    assert_eq!(t.execs[inner].argv[0], "/bin/sh");
    assert_eq!(t.execs[inner].parent, Some(0));
  }

  #[test]
  fn program_run_in_place_has_the_previous_program_as_parent() {
    let t = sh("exec /bin/true");
    assert_eq!(find(&t, "/bin/true").parent, Some(0));
  }

  #[test]
  fn programs_in_subshells_and_background_jobs_are_recorded() {
    let t = sh("( /bin/true a ) & /bin/true b; wait");
    assert!(t.execs.iter().any(|e| e.argv == ["/bin/true", "a"]));
    assert!(t.execs.iter().any(|e| e.argv == ["/bin/true", "b"]));
  }

  #[test]
  fn cleared_environment_does_not_escape() {
    let t = sh("/usr/bin/env -i /bin/true cleared");
    assert!(t.execs.iter().any(|e| e.argv == ["/bin/true", "cleared"]));
  }

  #[test]
  fn exit_status_is_returned() {
    let t = sh("exit 3");
    assert_eq!(t.status.code(), Some(3));
  }

  #[test]
  fn signals_reach_the_traced_program() {
    let t = sh("kill -TERM $$");
    assert_eq!(
      std::os::unix::process::ExitStatusExt::signal(&t.status),
      Some(libc::SIGTERM)
    );
  }

  #[test]
  fn background_program_is_reported_and_left_running() {
    let start = Instant::now();
    let t = sh("/bin/sleep 3 </dev/null >/dev/null 2>&1 &");
    assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
    // The job may not have started `sleep` yet when the shell exits.
    assert_eq!(t.still_running.len(), 1, "{:?}", t.still_running);
    let running = t.still_running[0].join(" ");
    assert!(running.contains("/bin/sleep 3"), "{running}");
  }

  #[test]
  fn programs_let_go_keep_running() {
    // Processes started just before the command exits are let go before
    // tracing has seen them start; none may be left stopped.
    let dir = tempfile::tempdir().unwrap();
    let done = dir.path().join("done");
    sh(&format!(
      "for i in $(seq 20); do \
         ( /bin/sleep 0.2; echo $i >> '{}' ) </dev/null >/dev/null 2>&1 & \
       done",
      done.display()
    ));
    let finished =
      || std::fs::read_to_string(&done).map_or(0, |s| s.lines().count());
    let start = Instant::now();
    while finished() < 20 && start.elapsed() < Duration::from_secs(5) {
      std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(finished(), 20);
  }

  #[test]
  fn missing_program_is_an_error() {
    let err = run(Command::new("/nonexistent/make")).unwrap_err();
    assert!(err.to_string().contains("/nonexistent/make"), "{err}");
  }

  #[test]
  fn tracing_does_not_reap_other_children() {
    // Tests run in threads of one process; tracing must only wait for its
    // own processes.
    let mut other = Command::new("/bin/sleep").arg("1").spawn().unwrap();
    sh("/bin/true");
    assert!(other.wait().unwrap().success());
  }
}
