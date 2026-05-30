//! Cooperative cancellation of the evaluator (Phase 5d of the notebook
//! interactive-server work). A coordinator installs an
//! `Arc<AtomicBool>` cancel flag via `Evaluator::with_cancel`; once it
//! reads `true`, execution returns `ScriptError::Cancelled` at the next
//! statement or loop-iteration boundary. Default-off: a fresh evaluator
//! never cancels, so the REPL / one-shot CLI are unaffected.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustlab_script::{Evaluator, ScriptError};

fn parse(src: &str) -> Vec<rustlab_script::ast::Stmt> {
    let tokens = rustlab_script::lexer::tokenize(src).unwrap();
    rustlab_script::parser::parse(tokens).unwrap()
}

#[test]
fn no_flag_means_never_cancels() {
    // Default evaluator: a bounded loop completes normally.
    let mut ev = Evaluator::new();
    let stmts = parse("s = 0; for k = 1:1000; s = s + k; end;");
    assert!(ev.run(&stmts).is_ok());
    assert_eq!(format!("{}", ev.get("s").unwrap()), "500500");
}

#[test]
fn pretripped_flag_cancels_a_bounded_loop() {
    // Flag already set before execution → the first For-iteration check
    // trips and the loop never runs to completion.
    let flag = Arc::new(AtomicBool::new(true));
    let mut ev = Evaluator::new().with_cancel(flag);
    let stmts = parse("s = 0; for k = 1:1000000; s = s + k; end; done = 1;");
    match ev.run(&stmts) {
        Err(ScriptError::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
    // The statement after the loop never ran.
    assert!(ev.get("done").is_none(), "execution continued past cancel");
}

#[test]
fn flag_interrupts_an_empty_body_infinite_loop() {
    // `while true; end;` has an empty body, so only the per-iteration
    // While check can stop it. Trip the flag from another thread and
    // confirm the run returns Cancelled (and does not hang).
    let flag = Arc::new(AtomicBool::new(false));
    let trip = flag.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        trip.store(true, Ordering::Relaxed);
    });

    let mut ev = Evaluator::new().with_cancel(flag);
    let stmts = parse("while true; end;");
    let start = Instant::now();
    let result = ev.run(&stmts);
    let elapsed = start.elapsed();
    handle.join().unwrap();

    assert!(
        matches!(result, Err(ScriptError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
    // Should stop promptly after the flag flips, not spin indefinitely.
    assert!(elapsed < Duration::from_secs(5), "loop took too long: {elapsed:?}");
}

#[test]
fn set_cancel_in_place_also_works() {
    let flag = Arc::new(AtomicBool::new(true));
    let mut ev = Evaluator::new();
    ev.set_cancel(flag);
    let stmts = parse("x = 1;");
    assert!(matches!(ev.run(&stmts), Err(ScriptError::Cancelled)));
}
