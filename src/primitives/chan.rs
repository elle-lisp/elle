//! Channel primitives — crossbeam-channel wrappers for inter-fiber messaging.

use std::cell::RefCell;
use std::time::Duration;

use crossbeam_channel::{self, TryRecvError, TrySendError};

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Newtype wrapper to satisfy crossbeam's `Send` requirement.
///
/// `Value` contains `Rc` (not `Send`). For single-threaded schedulers
/// (the common case) this is trivially safe. For cross-thread use the
/// scheduler is responsible for only sending immutable data.
struct SendableValue(Value);

// SAFETY: The scheduler contract guarantees that values sent through
// channels are either immutable or will not be accessed from the
// sending side after the send.
unsafe impl Send for SendableValue {}

/// Sender half of a channel, wrapped for `Value::external`.
struct ChanSender(RefCell<Option<crossbeam_channel::Sender<SendableValue>>>);

/// Receiver half of a channel, wrapped for `Value::external`.
struct ChanReceiver(RefCell<Option<crossbeam_channel::Receiver<SendableValue>>>);

/// Helper: extract `&ChanSender` from a Value or return a type error.
fn extract_sender<'a>(
    value: &'a Value,
    prim_name: &str,
) -> Result<&'a ChanSender, (SignalBits, Value)> {
    value.as_external::<ChanSender>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected chan/sender, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// Helper: extract `&ChanReceiver` from a Value or return a type error.
fn extract_receiver<'a>(
    value: &'a Value,
    prim_name: &str,
) -> Result<&'a ChanReceiver, (SignalBits, Value)> {
    value.as_external::<ChanReceiver>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected chan/receiver, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// `(chan)` or `(chan capacity)`
///
/// Returns `[sender receiver]` as an array.
fn prim_chan_new(args: &[Value]) -> (SignalBits, Value) {
    let (tx, rx) = match args.len() {
        0 => crossbeam_channel::unbounded(),
        1 => {
            let cap = match args[0].as_int() {
                Some(n) if n >= 0 => n as usize,
                Some(n) => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "value-error",
                            format!("chan: capacity must be non-negative, got {}", n),
                        ),
                    );
                }
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "chan: expected integer for capacity, got {}",
                                args[0].type_name()
                            ),
                        ),
                    );
                }
            };
            crossbeam_channel::bounded(cap)
        }
        n => {
            return (
                SIG_ERROR,
                error_val(
                    "arity-error",
                    format!("chan: expected 0 or 1 arguments, got {}", n),
                ),
            );
        }
    };

    let sender = Value::external("chan/sender", ChanSender(RefCell::new(Some(tx))));
    let receiver = Value::external("chan/receiver", ChanReceiver(RefCell::new(Some(rx))));
    (SIG_OK, Value::array(vec![sender, receiver]))
}

/// `(chan/send sender msg)` — non-blocking send.
///
/// Returns `[:ok]`, `[:full]`, or `[:disconnected]`.
fn prim_chan_send(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/send: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let sender = match extract_sender(&args[0], "chan/send") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    let tx = match inner.as_ref() {
        Some(tx) => tx,
        None => return (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
    };

    match tx.try_send(SendableValue(args[1])) {
        Ok(()) => (SIG_OK, Value::array(vec![Value::keyword("ok")])),
        Err(TrySendError::Full(_)) => (SIG_OK, Value::array(vec![Value::keyword("full")])),
        Err(TrySendError::Disconnected(_)) => {
            (SIG_OK, Value::array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/recv receiver)` — non-blocking receive.
///
/// Returns `[:ok msg]`, `[:empty]`, or `[:disconnected]`.
fn prim_chan_recv(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/recv: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let receiver = match extract_receiver(&args[0], "chan/recv") {
        Ok(r) => r,
        Err(e) => return e,
    };

    let inner = receiver.0.borrow();
    let rx = match inner.as_ref() {
        Some(rx) => rx,
        None => return (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
    };

    match rx.try_recv() {
        Ok(SendableValue(v)) => (SIG_OK, Value::array(vec![Value::keyword("ok"), v])),
        Err(TryRecvError::Empty) => (SIG_OK, Value::array(vec![Value::keyword("empty")])),
        Err(TryRecvError::Disconnected) => {
            (SIG_OK, Value::array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/clone sender)` — clone the sender half.
fn prim_chan_clone(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/clone: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let sender = match extract_sender(&args[0], "chan/clone") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    match inner.as_ref() {
        Some(tx) => {
            let cloned = tx.clone();
            (
                SIG_OK,
                Value::external("chan/sender", ChanSender(RefCell::new(Some(cloned)))),
            )
        }
        None => (
            SIG_ERROR,
            error_val("error", "chan/clone: sender is closed"),
        ),
    }
}

/// `(chan/close sender)` — close the sender half.
///
/// Drops the inner `Sender`, disconnecting the channel from this end.
fn prim_chan_close(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/close: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let sender = match extract_sender(&args[0], "chan/close") {
        Ok(s) => s,
        Err(e) => return e,
    };

    sender.0.borrow_mut().take();
    (SIG_OK, Value::NIL)
}

/// `(chan/close-recv receiver)` — close the receiver half.
///
/// Drops the inner `Receiver`, disconnecting the channel from this end.
fn prim_chan_close_recv(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/close-recv: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let receiver = match extract_receiver(&args[0], "chan/close-recv") {
        Ok(r) => r,
        Err(e) => return e,
    };

    receiver.0.borrow_mut().take();
    (SIG_OK, Value::NIL)
}

/// `(chan/select receivers)` or `(chan/select receivers timeout-ms)`
///
/// Blocks until one receiver has a message. Returns `[index msg]`.
/// With timeout, returns `[:timeout]` if no message arrives in time.
fn prim_chan_select(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("chan/select: expected 1 or 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Extract the array of receivers.
    let receivers_cell = match args[0].as_array_mut() {
        Some(arr) => arr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "chan/select: expected array of receivers, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let receivers_vec = receivers_cell.borrow();
    if receivers_vec.is_empty() {
        return (
            SIG_ERROR,
            error_val("value-error", "chan/select: receivers array is empty"),
        );
    }

    // Extract the inner crossbeam Receivers.
    let mut rxs: Vec<&crossbeam_channel::Receiver<SendableValue>> =
        Vec::with_capacity(receivers_vec.len());
    // We need to hold the borrows alive while we use the receivers.
    let mut borrows: Vec<std::cell::Ref<'_, Option<crossbeam_channel::Receiver<SendableValue>>>> =
        Vec::with_capacity(receivers_vec.len());

    for (i, val) in receivers_vec.iter().enumerate() {
        let chan_recv = match val.as_external::<ChanReceiver>() {
            Some(r) => r,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "chan/select: element {} is not a chan/receiver, got {}",
                            i,
                            val.external_type_name().unwrap_or(val.type_name())
                        ),
                    ),
                );
            }
        };
        borrows.push(chan_recv.0.borrow());
    }

    for (i, borrow) in borrows.iter().enumerate() {
        match borrow.as_ref() {
            Some(rx) => rxs.push(rx),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("chan/select: receiver at index {} is closed", i),
                    ),
                );
            }
        }
    }

    // Optional timeout.
    let timeout_ms = if args.len() == 2 {
        match args[1].as_int() {
            Some(ms) if ms >= 0 => Some(ms as u64),
            Some(ms) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("chan/select: timeout must be non-negative, got {}", ms),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "chan/select: expected integer for timeout, got {}",
                            args[1].type_name()
                        ),
                    ),
                );
            }
        }
    } else {
        None
    };

    // Build the Select set.
    let mut sel = crossbeam_channel::Select::new();
    for rx in &rxs {
        sel.recv(rx);
    }

    // Wait for a message.
    match timeout_ms {
        None => {
            let oper = sel.select();
            let index = oper.index();
            match oper.recv(rxs[index]) {
                Ok(SendableValue(v)) => (SIG_OK, Value::array(vec![Value::int(index as i64), v])),
                Err(_) => {
                    // Channel disconnected during select.
                    (SIG_OK, Value::array(vec![Value::keyword("disconnected")]))
                }
            }
        }
        Some(ms) => match sel.select_timeout(Duration::from_millis(ms)) {
            Ok(oper) => {
                let index = oper.index();
                match oper.recv(rxs[index]) {
                    Ok(SendableValue(v)) => {
                        (SIG_OK, Value::array(vec![Value::int(index as i64), v]))
                    }
                    Err(_) => (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
                }
            }
            Err(_) => (SIG_OK, Value::array(vec![Value::keyword("timeout")])),
        },
    }
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "chan",
        func: prim_chan_new,
        effect: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Create a channel. Returns [sender receiver]. Optional capacity for bounded channel.",
        params: &["&opt capacity"],
        category: "chan",
        example: "(chan)",
        aliases: &["chan/new"],
    },
    PrimitiveDef {
        name: "chan/send",
        func: prim_chan_send,
        effect: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Non-blocking send. Returns [:ok], [:full], or [:disconnected].",
        params: &["sender", "msg"],
        category: "chan",
        example: "(chan/send sender 42)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "chan/recv",
        func: prim_chan_recv,
        effect: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking receive. Returns [:ok msg], [:empty], or [:disconnected].",
        params: &["receiver"],
        category: "chan",
        example: "(chan/recv receiver)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "chan/clone",
        func: prim_chan_clone,
        effect: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Clone a sender. Multiple senders can feed the same channel.",
        params: &["sender"],
        category: "chan",
        example: "(chan/clone sender)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "chan/close",
        func: prim_chan_close,
        effect: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a sender. Receivers will get :disconnected after buffered messages drain.",
        params: &["sender"],
        category: "chan",
        example: "(chan/close sender)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "chan/close-recv",
        func: prim_chan_close_recv,
        effect: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a receiver. Senders will get :disconnected on next send.",
        params: &["receiver"],
        category: "chan",
        example: "(chan/close-recv receiver)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "chan/select",
        func: prim_chan_select,
        effect: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Block until one receiver has a message. Returns [index msg] or [:timeout].",
        params: &["receivers", "&opt timeout-ms"],
        category: "chan",
        example: "(chan/select @[r1 r2])",
        aliases: &[],
    },
];
