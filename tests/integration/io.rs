use crate::common::eval_source;

#[test]
fn test_io_request_predicate_false_on_int() {
    let result = eval_source("(io-request? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_request_predicate_false_on_string() {
    let result = eval_source("(io-request? \"hello\")").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_backend_predicate_false_on_int() {
    let result = eval_source("(io-backend? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_stream_read_line_outside_scheduler_errors() {
    // stream/read-line yields SIG_IO, which should error at top level
    let result = eval_source("(stream/read-line (port/open \"/dev/null\" :read))");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("SIG_IO") || err.contains("outside scheduler"),
        "expected SIG_IO error, got: {}",
        err
    );
}

#[test]
fn test_stream_write_outside_scheduler_errors() {
    let result = eval_source("(stream/write (port/stdout) \"hello\")");
    assert!(result.is_err());
}

#[test]
fn test_stream_write_non_port_errors() {
    // stream/write with a non-port should signal an error
    let result = eval_source("(stream/write 42 \"hello\")");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("type-error") || err.contains("port"),
        "expected type-error for non-port, got: {}",
        err
    );
}

#[test]
fn test_io_backend_sync() {
    let result = eval_source("(io-backend? (io/backend :sync))").unwrap();
    assert_eq!(result, elle::Value::bool(true));
}

#[test]
fn test_io_backend_invalid_kind() {
    let result = eval_source("(io/backend :invalid)");
    assert!(result.is_err());
}

#[test]
fn test_io_execute_roundtrip() {
    // Write a file, then read it back via io/execute
    let result = eval_source(
        "(begin
           (spit \"/tmp/elle-test-io-exec\" \"hello from elle\")
           (let* ((backend (io/backend :sync))
                  (port (port/open \"/tmp/elle-test-io-exec\" :read))
                  (f (fiber/new (fn [] (stream/read-all port)) 512)))
             (fiber/resume f)
             (io/execute backend (fiber/value f))))",
    )
    .unwrap();
    result
        .with_string(|s| assert_eq!(s, "hello from elle"))
        .unwrap();
}

#[test]
fn test_sync_scheduler_pure_fiber() {
    let result =
        eval_source("(sync-scheduler (fiber/new (fn [] (+ 1 2)) (bit/or 1 512)))").unwrap();
    assert_eq!(result.as_int(), Some(3));
}

#[test]
fn test_sync_scheduler_error_propagation() {
    let result =
        eval_source("(sync-scheduler (fiber/new (fn [] (error :test-error)) (bit/or 1 512)))");
    assert!(result.is_err());
}

#[test]
fn test_sync_scheduler_io_dispatch() {
    // Write a file, then read it via the scheduler
    let result = eval_source(
        "(begin
           (spit \"/tmp/elle-test-sched-io\" \"scheduler test\")
           (sync-scheduler
             (fiber/new
               (fn []
                 (let ((p (port/open \"/tmp/elle-test-sched-io\" :read)))
                   (stream/read-all p)))
               (bit/or 1 512))))",
    )
    .unwrap();
    result
        .with_string(|s| assert_eq!(s, "scheduler test"))
        .unwrap();
}

#[test]
fn test_scheduler_parameter_exists() {
    let result = eval_source("(parameter? *scheduler*)").unwrap();
    assert_eq!(result, elle::Value::bool(true));
}

#[test]
fn test_scheduler_parameter_default() {
    let result = eval_source("(= (*scheduler*) sync-scheduler)").unwrap();
    assert_eq!(result, elle::Value::bool(true));
}

#[test]
fn test_ev_spawn_pure() {
    let result = eval_source("(ev/spawn (fn [] 42))").unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_ev_spawn_with_io() {
    let result = eval_source(
        "(begin
           (spit \"/tmp/elle-test-ev-spawn\" \"spawn test\")
           (ev/spawn (fn []
             (let ((p (port/open \"/tmp/elle-test-ev-spawn\" :read)))
               (stream/read-all p)))))",
    )
    .unwrap();
    result.with_string(|s| assert_eq!(s, "spawn test")).unwrap();
}

#[test]
fn test_ev_spawn_error_propagation() {
    let result = eval_source("(ev/spawn (fn [] (error :boom)))");
    assert!(result.is_err());
}

// Chunk 7: Root fiber bootstrap tests

#[test]
fn test_pure_code_unchanged_with_scheduler() {
    // Pure code should work identically through the scheduler
    let result = eval_source("(+ 1 2 3)").unwrap();
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn test_stream_io_via_ev_spawn() {
    // With the scheduler bootstrap, stream I/O should work via ev/spawn
    let result = eval_source(
        "(begin
           (spit \"/tmp/elle-test-toplevel-io\" \"top level\")
           (ev/spawn (fn []
             (stream/read-all (port/open \"/tmp/elle-test-toplevel-io\" :read)))))",
    )
    .unwrap();
    result.with_string(|s| assert_eq!(s, "top level")).unwrap();
}

#[test]
fn test_existing_stdlib_functions_work() {
    // Verify stdlib functions still work (scheduler is transparent)
    let result = eval_source("(map (fn [x] (* x x)) (list 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_int(), Some(1));
    assert_eq!(vec[1].as_int(), Some(4));
    assert_eq!(vec[2].as_int(), Some(9));
}
