//! Process ids, and what the tracer does with them: waiting for, resuming and
//! inspecting traced processes. Linux only.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::Child;
use std::ptr;

use libc::c_int;
use libc::c_void;
use libc::pid_t;

/// The id of a process, or of one thread of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pid(pid_t);

impl From<&Child> for Pid {
  fn from(child: &Child) -> Pid {
    Pid(child.id() as pid_t)
  }
}

impl fmt::Display for Pid {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(f)
  }
}

/// Waits only for this thread's tracees: other threads of this process (such
/// as other tests) wait for their own children.
const WAIT_FLAGS: c_int = libc::__WALL | libc::__WNOTHREAD;

/// Makes the calling process traced by its parent. Meant to run in a child
/// just before it starts its program, which then stops it with `SIGTRAP`.
pub fn trace_me() -> io::Result<()> {
  if ptrace(libc::PTRACE_TRACEME, 0, 0) == -1 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

/// Waits for any traced process to change state; `None` once none are left.
pub fn wait_any() -> io::Result<Option<(Pid, c_int)>> {
  match waitpid(-1) {
    Ok(r) => Ok(Some(r)),
    Err(e) if e.raw_os_error() == Some(libc::ECHILD) => Ok(None),
    Err(e) => Err(e),
  }
}

impl Pid {
  /// Waits for this traced process to change state, and returns its status.
  pub fn wait(self) -> io::Result<c_int> {
    waitpid(self.0).map(|(_, status)| status)
  }

  /// Sets the tracing `options` (`PTRACE_O_*`) of this stopped process.
  pub fn set_options(self, options: c_int) -> io::Result<()> {
    if ptrace(libc::PTRACE_SETOPTIONS, self.0, options as usize) == -1 {
      return Err(io::Error::last_os_error());
    }
    Ok(())
  }

  pub fn resume(self) {
    self.resume_with_signal(0);
  }

  /// Resumes this stopped process, delivering `signal` (0 for none). A
  /// process that is gone already is not an error.
  pub fn resume_with_signal(self, signal: c_int) {
    ptrace(libc::PTRACE_CONT, self.0, signal as usize);
  }

  /// Stops tracing this stopped process.
  pub fn detach(self) {
    ptrace(libc::PTRACE_DETACH, self.0, 0);
  }

  /// Sends `SIGSTOP` to this thread alone.
  pub fn stop(self) {
    let tgid = self.tgid().unwrap_or(self);
    // SAFETY: tgkill only sends a signal.
    unsafe {
      libc::syscall(libc::SYS_tgkill, tgid.0, self.0, libc::SIGSTOP);
    }
  }

  /// The process id in the message of the event this process stopped at: a
  /// new process's id, or the former id of a thread that started a program.
  pub fn event_pid(self) -> Pid {
    let mut message: libc::c_ulong = 0;
    // SAFETY: PTRACE_GETEVENTMSG writes one c_ulong to `message`.
    unsafe {
      libc::ptrace(
        libc::PTRACE_GETEVENTMSG,
        self.0,
        ptr::null_mut::<c_void>(),
        &mut message as *mut libc::c_ulong as *mut c_void,
      );
    }
    Pid(message as pid_t)
  }

  /// Whether this process, stopped by `signal`, is stopping itself rather
  /// than being sent the signal: the kernel has no signal details for such a
  /// stop.
  pub fn is_group_stop(self, signal: c_int) -> bool {
    if ![libc::SIGSTOP, libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU]
      .contains(&signal)
    {
      return false;
    }
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
    // SAFETY: PTRACE_GETSIGINFO writes one siginfo_t to `info`.
    let r = unsafe {
      libc::ptrace(
        libc::PTRACE_GETSIGINFO,
        self.0,
        ptr::null_mut::<c_void>(),
        info.as_mut_ptr() as *mut c_void,
      )
    };
    r == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL)
  }

  /// The arguments of the program this process runs.
  pub fn cmdline(self) -> io::Result<Vec<String>> {
    let bytes = std::fs::read(self.proc("cmdline"))?;
    let mut argv: Vec<String> = bytes
      .split(|&b| b == 0)
      .map(|a| String::from_utf8_lossy(a).into_owned())
      .collect();
    // The arguments end with a NUL, which leaves an empty last item.
    if argv.last().is_some_and(String::is_empty) {
      argv.pop();
    }
    Ok(argv)
  }

  /// This process's working directory, or an empty path if it is unknown.
  pub fn cwd(self) -> PathBuf {
    std::fs::read_link(self.proc("cwd")).unwrap_or_default()
  }

  /// The executable this process runs, with symlinks resolved, or an empty
  /// path if it is unknown.
  pub fn exe(self) -> PathBuf {
    std::fs::read_link(self.proc("exe")).unwrap_or_default()
  }

  /// The process this thread belongs to (its own id for a main thread).
  pub fn tgid(self) -> Option<Pid> {
    self.status_field("Tgid:")
  }

  /// The parent of this process.
  pub fn ppid(self) -> Option<Pid> {
    self.status_field("PPid:")
  }

  fn status_field(self, name: &str) -> Option<Pid> {
    let status = std::fs::read_to_string(self.proc("status")).ok()?;
    status
      .lines()
      .find_map(|l| l.strip_prefix(name))
      .and_then(|v| v.trim().parse().ok())
      .map(Pid)
  }

  fn proc(self, name: &str) -> PathBuf {
    PathBuf::from(format!("/proc/{}/{name}", self.0))
  }
}

/// `ptrace(request, pid, 0, data)`.
fn ptrace(request: libc::c_uint, pid: pid_t, data: usize) -> libc::c_long {
  // SAFETY: none of the requests used read or write our memory through
  // `data`.
  unsafe {
    libc::ptrace(request, pid, ptr::null_mut::<c_void>(), data as *mut c_void)
  }
}

/// Waits for traced `pid` (or any, for -1) to change state.
fn waitpid(pid: pid_t) -> io::Result<(Pid, c_int)> {
  loop {
    let mut status = 0;
    // SAFETY: waitpid writes one c_int to `status`.
    let r = unsafe { libc::waitpid(pid, &mut status, WAIT_FLAGS) };
    if r != -1 {
      return Ok((Pid(r), status));
    }
    let e = io::Error::last_os_error();
    if e.kind() != io::ErrorKind::Interrupted {
      return Err(e);
    }
  }
}
