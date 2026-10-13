// audited: 2026-10-12
// A macro that never returns fails the compile that expands it: an expansion spends a budget apart from fiber fuel.
//
// docs/macros.md
// docs/runtime.md

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one run may take before the test calls it a hang. A transformer
/// that spends the whole expansion budget returns in well under a second on a
/// release build, and the rest is slack for a debug build on a loaded machine.
const DEADLINE: Duration = Duration::from_secs(120);

/// What one run of the binary did. `code` is `None` when the run passed
/// [`DEADLINE`] and the test killed it.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Run the elle binary in `cwd` with `args`, feed it `stdin`, and stop it at
/// [`DEADLINE`]. A thread drains each stream, so a run that fills a pipe
/// cannot stall against its own deadline.
fn run_elle(cwd: &Path, args: &[&str], stdin: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let drain = |mut stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stream.read_to_string(&mut text);
            text
        })
    };
    let stdout = drain(Box::new(child.stdout.take().expect("piped stdout")));
    let stderr = drain(Box::new(child.stderr.take().expect("piped stderr")));
    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().expect("poll elle") {
            break Some(status.code().unwrap_or(-1));
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Run {
        code,
        stdout: stdout.join().expect("stdout reader"),
        stderr: stderr.join().expect("stderr reader"),
    }
}

/// A macro whose body never returns.
const SPIN: &str = "(defmacro spin [] (forever nil))";

#[test]
fn a_looping_macro_fails_the_compile_of_a_file() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let file = dir.path().join("spin.lisp");
    std::fs::write(&file, format!("{SPIN}\n(spin)\n")).expect("write spin.lisp");
    let file = file.display().to_string();
    let run = run_elle(dir.path(), &[&file], "");
    assert_eq!(
        run.code,
        Some(1),
        "the compile fails, and no hang reaches the deadline: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("macro 'spin'") && run.stderr.contains("fuel"),
        "the error names the macro and its budget: {}",
        run.stderr
    );
}

#[test]
fn a_looping_macro_fails_eval() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let program = format!(
        "(let [[ok? err] (protect (eval '(begin {SPIN} (spin))))]\n  \
         (print (if ok? \"expanded\" (get err :message))))\n"
    );
    let run = run_elle(dir.path(), &["-"], &program);
    assert_eq!(
        run.code,
        Some(0),
        "eval raises, and protect catches it: {}",
        run.stderr
    );
    assert!(
        run.stdout.contains("macro 'spin'") && run.stdout.contains("fuel"),
        "the error names the macro and its budget: {}",
        run.stdout
    );
}

#[test]
fn a_looping_macro_fails_compile_analyze_of_its_file() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let program = format!(
        "(let [[ok? err] (protect (compile/analyze \"{SPIN} (spin)\"))]\n  \
         (print (if ok? \"analyzed\" (get err :message))))\n"
    );
    let run = run_elle(dir.path(), &["-"], &program);
    assert_eq!(run.code, Some(0), "the analysis raises: {}", run.stderr);
    assert!(
        run.stdout.contains("macro 'spin'") && run.stdout.contains("fuel"),
        "the error names the macro and its budget: {}",
        run.stdout
    );
}

/// The analysis of an importer expands only the importer. The counter-factual
/// is the analysis that compiled each literal import, which expanded the
/// module's `spin` and hung until the deadline.
#[test]
fn analyzing_an_importer_expands_no_macro_of_its_import() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let module = dir.path().join("m.lisp");
    std::fs::write(&module, format!("{SPIN}\n(fn [] {{:x (spin)}})\n")).expect("write m.lisp");
    let program = format!(
        "(compile/analyze \"(def m ((import-file \\\"{}\\\"))) m\")\n(print \"analyzed\")\n",
        module.display()
    );
    let run = run_elle(dir.path(), &["-"], &program);
    assert_eq!(run.code, Some(0), "the analysis returns: {}", run.stderr);
    assert_eq!(run.stdout.trim(), "analyzed");
}

#[test]
fn a_looping_begin_for_syntax_fails_the_compile() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let program =
        "(let [[ok? err] (protect (eval '(begin (begin-for-syntax (def x (forever nil))) 1)))]\n  \
                   (print (if ok? \"evaluated\" (get err :message))))\n";
    let run = run_elle(dir.path(), &["-"], program);
    assert_eq!(
        run.code,
        Some(0),
        "eval raises, and protect catches it: {}",
        run.stderr
    );
    assert!(
        run.stdout.contains("begin-for-syntax") && run.stdout.contains("fuel"),
        "the error names the definition and its budget: {}",
        run.stdout
    );
}

/// The budget is the expansion's own. A fiber whose fuel would run out
/// halfway through the transformer still gets the expansion, because the
/// transformer neither spends that fuel nor can pause when it runs out. The
/// counter-factual runs the transformer on the fiber's fuel, which runs out
/// inside it, and the expansion fails on the `:fuel` signal.
#[test]
fn a_transformer_does_not_spend_the_fibers_fuel() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let program = "(def f (fiber/new (fn [] (eval '(begin (defmacro count-up [] (begin (var i 0) (while (< i 5000) (assign i (+ i 1))) i)) (count-up))))\n\
                   |:fuel :error|))\n\
                   (fiber/set-fuel f 1000)\n\
                   (print (fiber/resume f))\n";
    let run = run_elle(dir.path(), &["-"], program);
    assert_eq!(run.code, Some(0), "the program runs: {}", run.stderr);
    assert_eq!(run.stdout.trim(), "5000", "the expansion answers its count");
}

/// The budget leaves room for real work: a transformer that loops a hundred
/// thousand times expands.
#[test]
fn a_macro_within_the_budget_expands() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let program =
        "(defmacro count-up [] (begin (var i 0) (while (< i 100000) (assign i (+ i 1))) i))\n\
                   (print (count-up))\n";
    let run = run_elle(dir.path(), &["-"], program);
    assert_eq!(run.code, Some(0), "the program runs: {}", run.stderr);
    assert_eq!(run.stdout.trim(), "100000");
}

/// A loop on a child fiber spends the expansion budget. `protect` runs its
/// body on a fiber of its own, and the loop's `+` call is not silent. The
/// counter-factual is a budget that only the expansion's own fiber spends: the
/// child holds no budget, so the loop runs until the deadline.
#[test]
fn a_loop_on_a_child_fiber_fails_the_compile() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let file = dir.path().join("child.lisp");
    let program = "(defmacro on-child []\n  \
                   (begin (protect (begin (var i 0) (while true (assign i (+ i 1))))) :caught))\n\
                   (print (on-child))\n";
    std::fs::write(&file, program).expect("write child.lisp");
    let file = file.display().to_string();
    let run = run_elle(dir.path(), &[&file], "");
    assert_eq!(run.code, Some(1), "the compile fails: {}", run.stderr);
    assert!(
        run.stderr.contains("macro 'on-child'") && run.stderr.contains("fuel"),
        "the error names the macro and its budget: {}",
        run.stderr
    );
}

/// A silent function the transformer calls spends the budget too, and the
/// halt crosses its boundary. `helper` holds no call, so its signal is silent.
/// The counter-factual is the silent spend outside an expansion, which never
/// stops the fiber, so the loop runs until the deadline.
#[test]
fn a_silent_loop_a_transformer_calls_fails_the_compile() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let file = dir.path().join("silent.lisp");
    let program = "(begin-for-syntax (def helper (fn [go] (while go nil) 0)))\n\
                   (defmacro spin-in-helper [] (helper true))\n\
                   (print (spin-in-helper))\n";
    std::fs::write(&file, program).expect("write silent.lisp");
    let file = file.display().to_string();
    let run = run_elle(dir.path(), &[&file], "");
    assert_eq!(run.code, Some(1), "the compile fails: {}", run.stderr);
    assert!(
        run.stderr.contains("macro 'spin-in-helper'") && run.stderr.contains("fuel"),
        "the error names the macro and its budget: {}",
        run.stderr
    );
}

/// A nested expansion spends the budget of the expansion that started it.
/// `outer` spends 9,000,000 units, then its `eval` expands `inner`, which
/// spends 9,000,000 more, past the 16,777,216 the two share, and halts. `spin`
/// is silent and charges one unit per trip. The counter-factual gives each
/// expansion a fresh budget, so `inner` fits, and the program prints
/// `:survived`.
///
/// The outer expansion runs through `eval`, because the macro VM a file
/// compiles on holds no compile context, and an `eval` there fails. The inner
/// form is read from a string when `outer` runs: the outer `eval` expands the
/// macro calls inside a quoted form ahead of time, which would run `inner`
/// before `outer` and outside its budget.
#[test]
fn a_nested_expansion_spends_the_outer_budget() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let spin = "(begin-for-syntax (def spin (fn [n] (numeric!) (def @i 0) \
                (while (%lt i n) (assign i (%add i 1))) i)))";
    let program = format!(
        r#"(def [ok? r]
  (protect (eval '(begin {spin}
    (defmacro outer []
      (begin (spin 9000000)
             (eval (read "(begin {spin} (defmacro inner [] (begin (spin 9000000) 1)) (inner))"))
             :survived))
    (outer)))))
(print (if ok? r (get r :message)))
"#
    );
    let run = run_elle(dir.path(), &["-"], &program);
    assert_eq!(run.code, Some(0), "the program runs: {}", run.stderr);
    assert!(
        run.stdout.contains("macro 'inner'") && run.stdout.contains("fuel"),
        "the inner expansion finds the shared budget spent: {}",
        run.stdout
    );
}
