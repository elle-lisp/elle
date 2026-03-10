use elle::context::{set_symbol_table, set_vm_context};
use elle::{compile_file, init_stdlib, register_primitives, SymbolTable, Value, VM};

/// Evaluate Elle source with `execute_scheduled` so SIG_IO is handled.
fn eval_scheduled(input: &str) -> Result<Value, String> {
    let (mut vm, mut symbols) = setup_scheduled();
    run_scheduled(input, &mut vm, &mut symbols)
}

/// Initialize a VM with primitives and stdlib for scheduled execution.
/// This is the expensive part (~4s) — call it before spawning helper threads.
fn setup_scheduled() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Compile and execute Elle source on an already-initialized VM.
fn run_scheduled(input: &str, vm: &mut VM, symbols: &mut SymbolTable) -> Result<Value, String> {
    let result = compile_file(input, symbols, "<test>")?;
    let value = vm.execute_scheduled(&result.bytecode, symbols)?;
    set_vm_context(std::ptr::null_mut());
    Ok(value)
}

// --- Minimal SIG_IO test ---

#[test]
fn test_stream_write_via_scheduled() {
    // Simplest possible SIG_IO roundtrip: write to /dev/null
    let result = eval_scheduled(
        r#"(let ((p (port/open "/dev/null" :write)))
             (stream/write p "hello")
             (port/close p)
             true)"#,
    );
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

// --- Scheduled I/O tests (TCP echo, etc.) ---

#[test]
fn test_tcp_echo_roundtrip() {
    // Listen on OS-assigned port, spawn a thread that connects and writes,
    // accept in Elle, read the line.
    use std::io::Write;

    // Do expensive VM setup before reserving the port.
    let (mut vm, mut symbols) = setup_scheduled();

    // Create listener in Rust to get the port number
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    drop(listener); // Free the port

    // Spawn a thread that retries connecting until the Elle listener is ready.
    let connect_thread = std::thread::spawn(move || {
        for _ in 0..50 {
            match std::net::TcpStream::connect(format!("127.0.0.1:{}", port)) {
                Ok(mut stream) => {
                    stream.write_all(b"hello-net\n").unwrap();
                    return;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        panic!("could not connect to 127.0.0.1:{}", port);
    });

    let code = format!(
        r#"(let* ((listener (tcp/listen "127.0.0.1" {port}))
               (conn (tcp/accept listener))
               (line (stream/read-line conn)))
          (port/close conn)
          (port/close listener)
          line)"#,
        port = port
    );

    let result = run_scheduled(&code, &mut vm, &mut symbols).unwrap();
    result.with_string(|s| assert_eq!(s, "hello-net")).unwrap();

    connect_thread.join().unwrap();
}

#[test]
fn test_udp_roundtrip() {
    // Bind two UDP sockets, send from A to B, recv on B.
    use std::net::UdpSocket;

    // Do expensive VM setup before reserving the port.
    let (mut vm, mut symbols) = setup_scheduled();

    let sock_b = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr_b = sock_b.local_addr().unwrap();
    let port_b = addr_b.port();
    // Drop the Rust socket so Elle can bind the same port.
    drop(sock_b);

    // Spawn a thread that sends repeatedly until joined.
    let send_thread = std::thread::spawn(move || {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        // Send repeatedly — the Elle side binds synchronously then
        // blocks on recv-from. One of these packets will arrive.
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = sock.send_to(b"udp-hello", format!("127.0.0.1:{}", port_b));
        }
    });

    let code = format!(
        r#"(let* ((sock (udp/bind "127.0.0.1" {port}))
               (result (udp/recv-from sock 1024)))
          (port/close sock)
          result)"#,
        port = port_b
    );

    let result = run_scheduled(&code, &mut vm, &mut symbols).unwrap();
    // Result is a struct with :data, :addr, :port
    let fields = result.as_struct().expect("expected struct result");
    use elle::value::heap::TableKey;
    let data = fields.get(&TableKey::Keyword("data".into())).unwrap();
    let data_bytes = data.as_bytes().unwrap();
    assert_eq!(data_bytes, b"udp-hello");

    send_thread.join().unwrap();
}

#[test]
fn test_unix_echo_roundtrip() {
    use std::io::Write;

    // Do expensive VM setup before creating the socket.
    let (mut vm, mut symbols) = setup_scheduled();

    let sock_path = format!("/tmp/elle-test-net-unix-{}.sock", std::process::id());
    let _ = std::fs::remove_file(&sock_path);

    let path_clone = sock_path.clone();
    let connect_thread = std::thread::spawn(move || {
        for _ in 0..50 {
            match std::os::unix::net::UnixStream::connect(&path_clone) {
                Ok(stream) => {
                    let mut writer = std::io::BufWriter::new(stream);
                    writer.write_all(b"unix-hello\n").unwrap();
                    return;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        panic!("could not connect to {}", path_clone);
    });

    let code = format!(
        r#"(let* ((listener (unix/listen "{path}"))
               (conn (unix/accept listener))
               (line (stream/read-line conn)))
          (port/close conn)
          (port/close listener)
          line)"#,
        path = sock_path
    );

    let result = run_scheduled(&code, &mut vm, &mut symbols).unwrap();
    result.with_string(|s| assert_eq!(s, "unix-hello")).unwrap();

    connect_thread.join().unwrap();
    std::fs::remove_file(&sock_path).ok();
}

#[test]
fn test_tcp_graceful_shutdown() {
    use std::io::Write;

    // Do expensive VM setup before reserving the port.
    let (mut vm, mut symbols) = setup_scheduled();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    drop(listener);

    let connect_thread = std::thread::spawn(move || {
        for _ in 0..50 {
            match std::net::TcpStream::connect(format!("127.0.0.1:{}", port)) {
                Ok(mut stream) => {
                    stream.write_all(b"before-shutdown\n").unwrap();
                    // Keep connection alive briefly
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    return;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        panic!("could not connect to 127.0.0.1:{}", port);
    });

    let code = format!(
        r#"(let* ((listener (tcp/listen "127.0.0.1" {port}))
               (conn (tcp/accept listener))
               (line (stream/read-line conn)))
          (tcp/shutdown conn :write)
          (port/close conn)
          (port/close listener)
          line)"#,
        port = port
    );

    let result = run_scheduled(&code, &mut vm, &mut symbols).unwrap();
    result
        .with_string(|s| assert_eq!(s, "before-shutdown"))
        .unwrap();

    connect_thread.join().unwrap();
}
