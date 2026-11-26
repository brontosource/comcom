//! Turns the programs a build ran into compile commands, and notes anything
//! that keeps those commands from being complete.

use std::path::Path;
use std::path::PathBuf;

use db::Entry;

use super::warnings::Warnings;

/// One program a traced build started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
  /// The program that started this one: either the program that ran in the
  /// same process before it, or the one running in the parent process when
  /// this process was created. `None` for the build tool itself.
  pub parent: Option<usize>,
  pub argv: Vec<String>,
  pub cwd: PathBuf,
  /// The executable that was actually run, with symlinks resolved.
  pub exe: PathBuf,
}

/// The compile commands found in a build, and why they might be incomplete.
#[derive(Debug, Default, PartialEq)]
pub struct Analysis {
  pub entries: Vec<Entry>,
  pub warnings: Warnings,
}

/// The compile commands among `execs` (in the order they ran), and warnings
/// for anything that may have compiled without leaving a command we can see.
pub fn analyze(execs: &[Exec]) -> Analysis {
  let kinds: Vec<Kind> = execs.iter().map(kind).collect();
  let ancestors =
    |i: usize| std::iter::successors(execs[i].parent, |&p| execs[p].parent);

  // Whether some program under each one compiled, or ran a compiler stage.
  let mut ran_compiler = vec![false; execs.len()];
  for (i, k) in kinds.iter().enumerate() {
    if *k != Kind::Other {
      for a in ancestors(i) {
        ran_compiler[a] = true;
      }
    }
  }

  let mut analysis = Analysis::default();
  for (i, e) in execs.iter().enumerate() {
    // A program under a compiler or wrapper is part of that compilation and can
    // be skipped.
    if ancestors(i).any(|a| kinds[a] != Kind::Other && kinds[a] != Kind::Stage)
    {
      continue;
    }
    let found = match kinds[i] {
      Kind::Compiler => compile(&e.argv, &e.cwd),
      Kind::Wrapper => match wrapped(&e.argv) {
        Some(inner) => compile(inner, &e.cwd),
        None => Compile::None,
      },
      Kind::Stage => {
        let by = e.parent.map_or("nothing", |p| {
          execs[p].argv.first().map_or("", String::as_str)
        });
        analysis.warnings.push(format!(
          "`{}` (part of a compiler) was started by `{by}` in {}, which \
           comcom does not recognize as a compiler, so its compile is \
           missing",
          e.argv.first().map_or("", String::as_str),
          e.cwd.display()
        ));
        Compile::None
      }
      Kind::Other => {
        if !ran_compiler[i] && looks_like_wrapper(&e.argv) {
          analysis.warnings.push(format!(
            "`{}` in {} looks like it compiles through a wrapper comcom does \
             not know, but no compiler ran under it (as on a cache hit), so \
             its compile may be missing",
            e.argv.join(" "),
            e.cwd.display()
          ));
        }
        Compile::None
      }
    };
    match found {
      Compile::Entries(entries) => analysis.entries.extend(entries),
      Compile::ResponseFile => analysis.warnings.push(format!(
        "`{}` in {} reads its arguments from a response file, so the files \
         it compiles could not be recorded",
        e.argv.join(" "),
        e.cwd.display()
      )),
      Compile::None => {}
    }
  }
  analysis
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
  /// A compiler driver, such as `gcc` or `clang`.
  Compiler,
  /// A compiler wrapper such as `ccache`. It may produce the output without
  /// running a compiler, so its own command is the one to record.
  Wrapper,
  /// A stage a compiler driver runs, such as GCC's `cc1` or `clang -cc1`.
  Stage,
  Other,
}

const WRAPPERS: &[&str] =
  &["ccache", "sccache", "distcc", "icecc", "buildcache"];
const STAGES: &[&str] = &["cc1", "cc1plus", "cc1obj", "cc1objplus"];
/// Compiler drivers. Besides the usual ones, these include Clang-based drivers
/// that some vendors ship under names of their own.
const COMPILERS: &[&str] = &[
  "gcc",
  "g++",
  "cc",
  "c++",
  "clang",
  "clang++",
  "icx",
  "icpx",
  "armclang",
  "amdclang",
  "amdclang++",
  "xlclang++",
];

fn kind(e: &Exec) -> Kind {
  let exe = file_name(&e.exe);
  let arg0 = e.argv.first().map_or("", |a| file_name(Path::new(a)));
  if WRAPPERS.contains(&exe) || WRAPPERS.contains(&arg0) {
    Kind::Wrapper
  } else if STAGES.contains(&exe) {
    Kind::Stage
  } else if is_compiler(exe) || is_compiler(arg0) {
    match e.argv.get(1).map(String::as_str) {
      Some("-cc1" | "-cc1as") => Kind::Stage,
      _ => Kind::Compiler,
    }
  } else {
    Kind::Other
  }
}

fn file_name(path: &Path) -> &str {
  path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

/// Whether `name` names a C or C++ compiler driver, possibly with a target
/// prefix (`aarch64-linux-gnu-gcc`) or a version suffix (`clang-19`).
fn is_compiler(name: &str) -> bool {
  let name = match name.rsplit_once('-') {
    Some((base, version))
      if !version.is_empty()
        && version.chars().all(|c| c.is_ascii_digit() || c == '.') =>
    {
      base
    }
    _ => name,
  };
  COMPILERS.iter().any(|c| {
    name == *c
      || name
        .strip_suffix(c)
        .is_some_and(|prefix| prefix.ends_with('-'))
  })
}

/// The compile command a wrapper runs: its own arguments when it is installed
/// under the compiler's name, else the arguments after its name. `None` for
/// the wrapper's own commands, such as `ccache -s`.
fn wrapped(argv: &[String]) -> Option<&[String]> {
  let arg0 = argv.first().map_or("", |a| file_name(Path::new(a)));
  if !WRAPPERS.contains(&arg0) {
    return Some(argv);
  }
  let rest = &argv[1..];
  match rest.first() {
    Some(first) if !first.starts_with('-') => Some(rest),
    _ => None,
  }
}

/// Whether `argv` runs some program that is given a compile command, as a
/// wrapper does.
fn looks_like_wrapper(argv: &[String]) -> bool {
  let arg0 = argv.first().map_or("", |a| file_name(Path::new(a)));
  if ["echo", "printf"].contains(&arg0) {
    return false;
  }
  (1..argv.len()).any(|i| {
    is_compiler(file_name(Path::new(&argv[i])))
      && matches!(compile(&argv[i..], Path::new("")), Compile::Entries(e) if !e.is_empty())
  })
}

enum Compile {
  Entries(Vec<Entry>),
  /// Some arguments are in a response file, so the sources are unknown.
  ResponseFile,
  None,
}

/// Options whose value is the next argument.
const TAKES_VALUE: &[&str] = &[
  "-o",
  "-I",
  "-D",
  "-U",
  "-include",
  "-imacros",
  "-isystem",
  "-iquote",
  "-idirafter",
  "-iprefix",
  "-iwithprefix",
  "-iwithprefixbefore",
  "-isysroot",
  "-imultilib",
  "-MF",
  "-MT",
  "-MQ",
  "-MJ",
  "-Xclang",
  "-Xlinker",
  "-Xpreprocessor",
  "-Xassembler",
  "-mllvm",
  "-L",
  "-l",
  "-T",
  "-u",
  "-e",
  "-z",
  "-B",
  "-F",
  "-arch",
  "-target",
  "-framework",
  "-aux-info",
  "-dumpbase",
  "-dumpdir",
  "--param",
  "--sysroot",
];

/// Options after which the compiler does not compile.
const NO_COMPILE: [&str; 4] = ["-E", "-M", "-MM", "-fsyntax-only"];

const SOURCE_EXTENSIONS: [&str; 10] =
  ["c", "cc", "cp", "cpp", "CPP", "cxx", "c++", "C", "m", "mm"];
const SOURCE_LANGUAGES: [&str; 4] =
  ["c", "c++", "objective-c", "objective-c++"];

/// The entries for the compiler command `argv` run in `cwd`: one per source,
/// each with the other sources left out of its arguments.
fn compile(argv: &[String], cwd: &Path) -> Compile {
  let mut sources = Vec::new();
  let mut output = None;
  let mut compile_only = false;
  let mut response_file = false;
  // The language set by `-x`: `None` before any, or after `-x none`.
  let mut language: Option<&str> = None;
  let mut i = 1;
  while i < argv.len() {
    let arg = argv[i].as_str();
    if NO_COMPILE.contains(&arg) {
      return Compile::None;
    }
    if arg == "-x" {
      language = argv.get(i + 1).map(String::as_str).filter(|l| *l != "none");
      i += 2;
      continue;
    }
    if TAKES_VALUE.contains(&arg) {
      if arg == "-o" {
        output = argv.get(i + 1);
      }
      i += 2;
      continue;
    }
    if arg == "-c" || arg == "-S" {
      compile_only = true;
    } else if arg.starts_with('@') {
      response_file = true;
    } else if !arg.starts_with('-') {
      let is_source = match language {
        Some(l) => SOURCE_LANGUAGES.contains(&l),
        None => Path::new(arg)
          .extension()
          .and_then(|x| x.to_str())
          .is_some_and(|x| SOURCE_EXTENSIONS.contains(&x)),
      };
      if is_source {
        sources.push(i);
      }
    }
    i += 1;
  }
  if sources.is_empty() && response_file {
    return Compile::ResponseFile;
  }
  let output = output
    .filter(|_| compile_only && sources.len() == 1)
    .map(PathBuf::from);
  Compile::Entries(
    sources
      .iter()
      .map(|&s| Entry {
        directory: cwd.to_path_buf(),
        file: PathBuf::from(&argv[s]),
        arguments: argv
          .iter()
          .enumerate()
          .filter(|(j, _)| *j == s || !sources.contains(j))
          .map(|(_, a)| a.clone())
          .collect(),
        output: output.clone(),
      })
      .collect(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn exec(parent: Option<usize>, argv: &[&str]) -> Exec {
    exec_of(parent, argv, argv[0])
  }

  /// An `Exec` whose executable (after resolving symlinks) is `exe`.
  fn exec_of(parent: Option<usize>, argv: &[&str], exe: &str) -> Exec {
    Exec {
      parent,
      argv: argv.iter().map(|s| s.to_string()).collect(),
      cwd: PathBuf::from("/src"),
      exe: PathBuf::from("/usr/bin").join(exe),
    }
  }

  fn make() -> Exec {
    exec(None, &["make", "-B"])
  }

  fn entry(file: &str, args: &[&str], output: Option<&str>) -> Entry {
    Entry {
      directory: PathBuf::from("/src"),
      file: PathBuf::from(file),
      arguments: args.iter().map(|s| s.to_string()).collect(),
      output: output.map(PathBuf::from),
    }
  }

  #[test]
  fn compile_becomes_an_entry() {
    let a =
      analyze(&[make(), exec(Some(0), &["gcc", "-c", "a.c", "-o", "a.o"])]);
    assert_eq!(
      a.entries,
      [entry("a.c", &["gcc", "-c", "a.c", "-o", "a.o"], Some("a.o"))]
    );
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
  }

  #[test]
  fn compiler_is_recognized_by_resolved_executable() {
    let a = analyze(&[
      make(),
      exec_of(Some(0), &["mycc", "-c", "a.c"], "x86_64-linux-gnu-gcc-13"),
    ]);
    assert_eq!(a.entries.len(), 1);
  }

  #[test]
  fn compiler_names_are_recognized() {
    for name in [
      "cc",
      "c++",
      "gcc",
      "g++",
      "clang",
      "clang++",
      "gcc-14",
      "clang-19",
      "clang++-19.1",
      "aarch64-linux-gnu-gcc",
      "arm-none-eabi-g++-13",
      "icx",
      "icpx",
      "armclang",
      "amdclang++",
    ] {
      let a = analyze(&[make(), exec(Some(0), &[name, "-c", "a.c"])]);
      assert_eq!(a.entries.len(), 1, "{name}");
    }
  }

  #[test]
  fn similar_names_are_not_compilers() {
    for name in ["distccd", "gcc-ar", "clang-format", "c++filt", "tcc-wrap"] {
      let a = analyze(&[make(), exec(Some(0), &[name, "-c", "a.c"])]);
      assert!(a.entries.is_empty(), "{name}");
    }
  }

  #[test]
  fn compile_and_link_makes_one_entry_per_source() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["cc", "-O2", "a.c", "b.cc", "x.o", "-o", "prog"]),
    ]);
    assert_eq!(
      a.entries,
      [
        entry("a.c", &["cc", "-O2", "a.c", "x.o", "-o", "prog"], None),
        entry("b.cc", &["cc", "-O2", "b.cc", "x.o", "-o", "prog"], None),
      ]
    );
  }

  #[test]
  fn option_values_are_not_sources() {
    let a = analyze(&[
      make(),
      exec(
        Some(0),
        &["cc", "-include", "pre.c", "-MF", "a.d", "-o", "a.o", "-c", "a.c"],
      ),
    ]);
    assert_eq!(a.entries.len(), 1);
    assert_eq!(a.entries[0].file, PathBuf::from("a.c"));
  }

  #[test]
  fn language_flag_makes_any_file_a_source() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["cc", "-x", "c", "gen.inc", "-x", "none", "lib.a"]),
    ]);
    assert_eq!(a.entries.len(), 1);
    assert_eq!(a.entries[0].file, PathBuf::from("gen.inc"));
  }

  #[test]
  fn header_language_is_not_a_compile() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["cc", "-x", "c++-header", "pch.h", "-o", "pch.gch"]),
    ]);
    assert!(a.entries.is_empty());
  }

  #[test]
  fn runs_that_do_not_compile_make_no_entries() {
    for flag in ["-E", "-M", "-MM", "-fsyntax-only"] {
      let a = analyze(&[make(), exec(Some(0), &["cc", flag, "a.c"])]);
      assert!(a.entries.is_empty(), "{flag}");
    }
  }

  #[test]
  fn dependency_flags_alongside_a_compile_still_compile() {
    let a = analyze(&[make(), exec(Some(0), &["cc", "-MMD", "-c", "a.c"])]);
    assert_eq!(a.entries.len(), 1);
  }

  #[test]
  fn compiler_queries_make_no_entries() {
    let a = analyze(&[make(), exec(Some(0), &["gcc", "-dumpmachine"])]);
    assert!(a.entries.is_empty());
    assert!(a.warnings.is_empty());
  }

  #[test]
  fn programs_a_compiler_starts_are_part_of_its_compile() {
    // gcc runs cc1 and as; an out-of-process clang re-runs itself as -cc1.
    let a = analyze(&[
      make(),
      exec(Some(0), &["gcc", "-c", "a.c"]),
      exec_of(Some(1), &["/usr/libexec/gcc/cc1", "a.c"], "cc1"),
      exec(Some(1), &["as", "-o", "a.o"]),
      exec(Some(0), &["clang", "-c", "b.c"]),
      exec(Some(4), &["clang", "-cc1", "-emit-obj", "b.c"]),
    ]);
    let files: Vec<_> = a.entries.iter().map(|e| e.file.clone()).collect();
    assert_eq!(files, [PathBuf::from("a.c"), PathBuf::from("b.c")]);
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
  }

  #[test]
  fn compiler_under_a_shell_is_recorded() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["/bin/sh", "-c", "cd sub && cc -c a.c"]),
      Exec {
        cwd: PathBuf::from("/src/sub"),
        ..exec(Some(1), &["cc", "-c", "a.c"])
      },
    ]);
    assert_eq!(a.entries.len(), 1);
    assert_eq!(a.entries[0].directory, PathBuf::from("/src/sub"));
  }

  #[test]
  fn wrapper_is_recorded_without_the_wrapper() {
    // On a cache hit, no compiler runs at all.
    for wrapper in ["ccache", "sccache", "distcc", "icecc", "buildcache"] {
      let a = analyze(&[make(), exec(Some(0), &[wrapper, "gcc", "-c", "a.c"])]);
      assert_eq!(
        a.entries,
        [entry("a.c", &["gcc", "-c", "a.c"], None)],
        "{wrapper}"
      );
    }
  }

  #[test]
  fn wrapper_compiler_needs_no_recognizable_name() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["ccache", "/opt/x/mycc", "-c", "a.c"]),
    ]);
    assert_eq!(a.entries.len(), 1);
  }

  #[test]
  fn compiler_a_wrapper_starts_is_not_recorded_again() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["ccache", "gcc", "-c", "a.c"]),
      exec(Some(1), &["gcc", "-fdiagnostics-color", "-c", "a.c"]),
    ]);
    assert_eq!(a.entries, [entry("a.c", &["gcc", "-c", "a.c"], None)]);
  }

  #[test]
  fn wrapper_named_as_the_compiler_is_recorded_as_is() {
    // ccache installed as a `gcc` symlink.
    let a =
      analyze(&[make(), exec_of(Some(0), &["gcc", "-c", "a.c"], "ccache")]);
    assert_eq!(a.entries, [entry("a.c", &["gcc", "-c", "a.c"], None)]);
  }

  #[test]
  fn wrapper_admin_commands_are_not_compiles() {
    let a = analyze(&[make(), exec(Some(0), &["ccache", "-s"])]);
    assert!(a.entries.is_empty());
    assert!(a.warnings.is_empty());
  }

  #[test]
  fn compiler_stage_outside_a_compiler_is_a_warning() {
    // A gcc we did not recognize ran cc1.
    let a = analyze(&[
      make(),
      exec(Some(0), &["weird-gcc-name", "-c", "a.c"]),
      exec_of(Some(1), &["/usr/libexec/gcc/cc1plus", "a.c"], "cc1plus"),
    ]);
    assert!(a.entries.is_empty());
    assert_eq!(a.warnings.len(), 1);
    assert!(
      a.warnings.in_order()[0].contains("cc1plus"),
      "{}",
      a.warnings.in_order()[0]
    );
    assert!(
      a.warnings.in_order()[0].contains("weird-gcc-name"),
      "{}",
      a.warnings.in_order()[0]
    );
  }

  #[test]
  fn possible_wrapper_with_no_compiler_under_it_is_a_warning() {
    let a = analyze(&[make(), exec(Some(0), &["mycache", "gcc", "-c", "a.c"])]);
    assert!(a.entries.is_empty());
    assert_eq!(a.warnings.len(), 1);
    assert!(
      a.warnings.in_order()[0].contains("mycache gcc -c a.c"),
      "{}",
      a.warnings.in_order()[0]
    );
  }

  #[test]
  fn possible_wrapper_that_ran_a_compiler_is_fine() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["env", "FOO=1", "gcc", "-c", "a.c"]),
      exec(Some(1), &["gcc", "-c", "a.c"]),
    ]);
    assert_eq!(a.entries.len(), 1);
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
  }

  #[test]
  fn echo_is_not_a_possible_wrapper() {
    let a =
      analyze(&[make(), exec(Some(0), &["/bin/echo", "gcc", "-c", "a.c"])]);
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
  }

  #[test]
  fn response_file_compile_is_a_warning() {
    let a = analyze(&[make(), exec(Some(0), &["gcc", "@args.rsp"])]);
    assert!(a.entries.is_empty());
    assert_eq!(a.warnings.len(), 1);
    assert!(
      a.warnings.in_order()[0].contains("@args.rsp"),
      "{}",
      a.warnings.in_order()[0]
    );
  }

  #[test]
  fn identical_warnings_are_reported_once() {
    let a = analyze(&[
      make(),
      exec(Some(0), &["mycache", "gcc", "-c", "a.c"]),
      exec(Some(0), &["mycache", "gcc", "-c", "a.c"]),
    ]);
    assert_eq!(a.warnings.len(), 1);
  }

  #[test]
  fn path_has_no_file_name() {
    // An `Exec` for `/` must not panic.
    let a = analyze(&[Exec {
      exe: PathBuf::from("/"),
      ..exec(None, &["/"])
    }]);
    assert!(a.entries.is_empty());
  }
}
