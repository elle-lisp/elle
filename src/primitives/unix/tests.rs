//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::fiber::{SIG_IO, SIG_YIELD};

#[test]
fn test_unix_listen_returns_ok() {
    crate::value::arena::with_test_region(|| {
        let path = format!("/dev/shm/elle-test-unix-listen-{}.sock", std::process::id());
        // `val` is born in the ctx's region, which is released when
        // `with_test_ctx` returns — so inspect and close it inside the ctx
        // rather than dereferencing a freed slot afterward.
        let bits = crate::primitives::ctx::with_test_ctx(|ctx| {
            let arg = ctx.string(&*path);
            let (bits, val) = prim_unix_listen(ctx, &[arg]);
            let port = val.as_external::<Port>().unwrap();
            assert_eq!(port.kind(), PortKind::UnixListener);
            port.close();
            bits
        });
        assert_eq!(bits, SIG_OK);
        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_unix_accept_returns_sig_io() {
    crate::value::arena::with_test_region(|| {
        let path = format!("/dev/shm/elle-test-unix-accept-{}.sock", std::process::id());
        // `with_test_ctx` releases its region on return, so a Value born in
        // it must not be used afterward. The listener lives in the ctx's
        // region, so listen + accept + close all happen inside one ctx —
        // splitting them across two ctxs frees the listener's slot under
        // the second ctx and derefs a dangling Value (the prior UAF).
        let bits = crate::primitives::ctx::with_test_ctx(|ctx| {
            let arg = ctx.string(&*path);
            let (_, listener) = prim_unix_listen(ctx, &[arg]);
            let (bits, _) = prim_unix_accept(ctx, &[listener]);
            listener.as_external::<Port>().unwrap().close();
            bits
        });
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_unix_connect_returns_sig_io() {
    crate::value::arena::with_test_region(|| {
        let (bits, _) = crate::primitives::ctx::with_test_ctx(|ctx| {
            let arg = ctx.string("/dev/shm/nonexistent.sock");
            prim_unix_connect(ctx, &[arg])
        });
        assert_eq!(bits, SIG_YIELD | SIG_IO);
    });
}

#[test]
fn test_unix_shutdown_returns_sig_io() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        let stream_port = h
            .ctx()
            .external("port", Port::new_unix_stream(fd, "x".into()));
        let (bits, _) = crate::primitives::ctx::with_test_ctx(|ctx| {
            prim_unix_shutdown(ctx, &[stream_port, Value::keyword("read-write")])
        });
        assert_eq!(bits, SIG_YIELD | SIG_IO);
    });
}
