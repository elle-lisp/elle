// audited: 2026-09-29
//! A parameter crosses the send boundary with its id and default, and a stdio port by its kind.
//!
//! docs/threads.md

use super::*;

// ── sendable parameters + stdio ports ────────────────────────────────

#[test]
fn parameter_round_trips_preserving_id_and_default() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let p = h.ctx().parameter(Value::int(7));
        let (id0, _) = p.as_parameter().expect("p is a parameter");

        let bundle = SendBundle::from_value(p, h.heap(), None)
            .expect("a parameter with a sendable default must be sendable");
        let p2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        let (id1, def1) = p2
            .as_parameter()
            .expect("reconstructed value is a parameter");
        // Resolution is by id — the worker must see the same parameter.
        assert_eq!(
            id0, id1,
            "parameter id must be preserved across the boundary"
        );
        assert_eq!(def1.as_int(), Some(7), "default must round-trip");
    });
}

#[test]
fn stdio_port_round_trips_by_kind() {
    use crate::port::{Port, PortKind};
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        for (mk, kind) in [
            (Port::stdout as fn() -> Port, PortKind::Stdout),
            (Port::stderr as fn() -> Port, PortKind::Stderr),
            (Port::stdin as fn() -> Port, PortKind::Stdin),
        ] {
            let v = h.ctx().external("port", mk());
            let bundle =
                SendBundle::from_value(v, h.heap(), None).expect("stdio ports are sendable");
            let v2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));
            let got = v2.as_external::<Port>().map(|p| p.kind());
            assert_eq!(got, Some(kind), "stdio port must reconstruct with its kind");
        }
    });
}

#[test]
fn parameter_holding_stdout_port_is_sendable() {
    // `*stdout*` is `(parameter (port/stdout))`; this is the exact shape a
    // `println`-using closure closes over. It must serialize.
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let p = h
            .ctx()
            .parameter(h.ctx().external("port", crate::port::Port::stdout()));
        let bundle = SendBundle::from_value(p, h.heap(), None)
            .expect("a parameter defaulting to a stdio port must be sendable");
        let p2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));
        let (_, def) = p2.as_parameter().expect("reconstructed is a parameter");
        assert_eq!(
            def.as_external::<crate::port::Port>().map(|p| p.kind()),
            Some(crate::port::PortKind::Stdout),
            "the parameter's default stdout port must round-trip"
        );
    });
}
