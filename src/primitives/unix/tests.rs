//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::fiber::SIG_IO;

/// A short socket path under the platform temp root.
///
/// `/dev/shm` is a Linux tmpfs and does not exist on macOS or the BSDs. The
/// name is kept short on purpose: a `sockaddr_un` path is limited to about
/// 104 bytes, and the macOS temp root is already long.
fn sock_path(tag: &str) -> String {
    std::env::temp_dir()
        .join(format!("elle-{}-{}.sock", tag, std::process::id()))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn test_unix_listen_returns_ok() {
    crate::value::arena::with_test_region(|| {
        let path = sock_path("listen");
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
        let path = sock_path("accept");
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
        assert_eq!(bits, SIG_IO);
        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn test_unix_connect_returns_sig_io() {
    crate::value::arena::with_test_region(|| {
        let (bits, _) = crate::primitives::ctx::with_test_ctx(|ctx| {
            let arg = ctx.string(&*sock_path("nonexistent"));
            prim_unix_connect(ctx, &[arg])
        });
        assert_eq!(bits, SIG_IO);
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
        assert_eq!(bits, SIG_IO);
    });
}
