//! Elle TLS plugin — TLS state machine primitives via rustls.
//!
//! This plugin exposes rustls's UnbufferedClientConnection /
//! UnbufferedServerConnection as pure state machine primitives.
//! All socket I/O is performed in Elle code using stream/read and
//! stream/write on native TCP ports. No I/O happens in this plugin.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::error_val;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{TableKey, Value};
use rustls::client::UnbufferedClientConnection;
use rustls::server::UnbufferedServerConnection;
use rustls::unbuffered::{ConnectionState, UnbufferedStatus};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_native_certs::load_native_certs;
use rustls_pemfile::{certs, private_key};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// State structs
// ---------------------------------------------------------------------------

/// The TLS connection: client or server side.
pub enum TlsConnection {
    Client(UnbufferedClientConnection),
    Server(UnbufferedServerConnection),
}

/// ExternalObject wrapping a rustls state machine plus its I/O buffers.
///
/// Type name: "tls-state"
///
/// Buffer lifecycle:
///   incoming  — ciphertext from network, not yet processed by rustls
///   outgoing  — ciphertext produced by rustls, to be sent to network
///   plaintext — decrypted app data, to be read by Elle
pub struct TlsState {
    conn: RefCell<TlsConnection>,
    incoming: RefCell<Vec<u8>>,
    outgoing: RefCell<Vec<u8>>,
    plaintext: RefCell<Vec<u8>>,
    handshake_complete: Cell<bool>,
    /// When true, the next WriteTraffic state encountered in the drive loop
    /// will encode a close_notify alert into the outgoing buffer and clear
    /// this flag. Set by prim_tls_close_notify before driving.
    close_notify_pending: Cell<bool>,
}

/// ExternalObject wrapping a rustls ServerConfig.
///
/// Type name: "tls-server-config"
pub struct TlsServerConfig {
    config: Arc<ServerConfig>,
}

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

/// Plugin entry point. Called by Elle when loading the `.so`.
///
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
#[no_mangle]
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    // Install the ring crypto provider globally. Second call is a no-op (returns Err).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short = def.name.strip_prefix("tls/").unwrap_or(def.name);
        fields.insert(TableKey::Keyword(short.into()), Value::native_fn(def.func));
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract TlsState from args[idx], returning a type-error on failure.
fn get_tls_state<'a>(
    args: &'a [Value],
    idx: usize,
    name: &str,
) -> Result<&'a TlsState, (SignalBits, Value)> {
    args[idx].as_external::<TlsState>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected tls-state, got {}",
                    name,
                    args[idx].type_name()
                ),
            ),
        )
    })
}

/// Construct a tls-error signal value.
fn tls_err(name: &str, msg: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("tls-error", format!("{}: {}", name, msg)),
    )
}

/// Construct an io-error signal value.
fn io_err(name: &str, msg: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("io-error", format!("{}: {}", name, msg)),
    )
}

/// Build a ClientConfig. Loads system CA roots, falls back to webpki-roots.
fn build_client_config(
    no_verify: bool,
    ca_file: Option<&str>,
) -> Result<Arc<ClientConfig>, String> {
    if no_verify {
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();
        return Ok(Arc::new(config));
    }

    let mut root_store = RootCertStore::empty();

    if let Some(path) = ca_file {
        let data = std::fs::read(path).map_err(|e| format!("ca-file: {}", e))?;
        let mut reader = Cursor::new(&data);
        for cert in certs(&mut reader) {
            let cert = cert.map_err(|e| format!("ca-file PEM error: {}", e))?;
            root_store
                .add(cert)
                .map_err(|e| format!("ca-file cert error: {}", e))?;
        }
    } else {
        let native_result = load_native_certs();
        let loaded: Vec<_> = native_result.certs;

        if loaded.is_empty() {
            // Fall back to webpki-roots (Mozilla bundle).
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        } else {
            for cert in loaded {
                root_store
                    .add(cert)
                    .map_err(|e| format!("native cert error: {}", e))?;
            }
        }
    }

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Custom certificate verifier that skips all verification.
/// Used only when :no-verify is true. Never use in production.
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// ---------------------------------------------------------------------------
// Drive loop helper: drive state machine with incoming bytes.
// ---------------------------------------------------------------------------

/// Process one `ConnectionState` from the state machine loop.
///
/// Returns:
///   `Ok(None)`         — continue looping (EncodeTlsData, TransmitTlsData, ReadTraffic)
///   `Ok(Some(status))` — stop, return this status keyword
///   `Err(e)`           — propagate error signal
///
/// This macro is used by `drive_state_machine` to handle the per-Data-type
/// `ConnectionState` variants without duplicating the match body.
macro_rules! handle_conn_state {
    ($conn_state:expr, $outgoing:expr, $plaintext:expr, $handshake_done:expr, $state:expr) => {{
        match $conn_state {
            ConnectionState::EncodeTlsData(mut encode) => {
                let start = $outgoing.len();
                $outgoing.resize(start + 16_640, 0u8);
                let written = match encode.encode(&mut $outgoing[start..]) {
                    Ok(w) => w,
                    Err(e) => return Err(tls_err("tls/process", format!("encode error: {}", e))),
                };
                $outgoing.truncate(start + written);
                None // continue
            }
            ConnectionState::TransmitTlsData(transmit) => {
                transmit.done();
                None // continue
            }
            ConnectionState::BlockedHandshake => Some("handshaking"),
            ConnectionState::ReadTraffic(mut read_traffic) => {
                while let Some(record) = read_traffic.next_record() {
                    match record {
                        Ok(app_data) => $plaintext.extend_from_slice(app_data.payload),
                        Err(e) => {
                            return Err(tls_err(
                                "tls/process",
                                format!("read_traffic error: {}", e),
                            ))
                        }
                    }
                }
                Some("has-data")
            }
            ConnectionState::WriteTraffic(mut wt) => {
                $handshake_done.set(true);
                // If a close_notify was requested, encode it now into the outgoing
                // buffer. The flag is set by prim_tls_close_notify before it calls
                // drive_state_machine. WriteTraffic is the only state where we have
                // a mutable connection handle that can call queue_close_notify.
                if $state.close_notify_pending.get() {
                    $state.close_notify_pending.set(false);
                    let start = $outgoing.len();
                    // A close_notify alert is a 31-byte TLS record.
                    $outgoing.resize(start + 64, 0u8);
                    match wt.queue_close_notify(&mut $outgoing[start..]) {
                        Ok(written) => $outgoing.truncate(start + written),
                        Err(_) => $outgoing.truncate(start),
                    }
                }
                Some("ready")
            }
            ConnectionState::PeerClosed => Some("peer-closed"),
            ConnectionState::Closed => Some("closed"),
            _ => Some("handshaking"),
        }
    }};
}

/// Advance the TLS state machine with new incoming bytes.
///
/// Appends `new_data` to the internal incoming buffer, then loops through
/// `process_tls_records` until the state machine blocks or completes.
///
/// Returns a status keyword: "handshaking", "ready", "has-data",
/// "peer-closed", or "closed". On error returns Err with a signal tuple.
///
/// # Borrow discipline
///
/// `process_tls_records` borrows `incoming` via lifetime `'i` and returns a
/// `ConnectionState<'c, 'i, Data>` that keeps that borrow alive. The drain
/// of consumed bytes must therefore happen *after* `ConnectionState` is fully
/// handled and dropped. This function collects `(discard, result)` as a pair
/// so the state is dropped before the drain.
fn drive_state_machine(
    state: &TlsState,
    new_data: &[u8],
) -> Result<&'static str, (SignalBits, Value)> {
    state.incoming.borrow_mut().extend_from_slice(new_data);

    let mut conn = state.conn.borrow_mut();
    let mut incoming = state.incoming.borrow_mut();
    let mut outgoing = state.outgoing.borrow_mut();
    let mut plaintext = state.plaintext.borrow_mut();

    loop {
        // Process one round. We extract (discard, conn_state_result) in a block
        // so that ConnectionState<'_, '_, Data> is dropped before the drain call.
        // The macro handle_conn_state! may use `return Err(...)` — that's fine
        // because the macro is expanded inline, not inside a closure.
        macro_rules! one_round {
            ($raw_conn:expr) => {{
                let UnbufferedStatus { discard, state: cs } =
                    $raw_conn.process_tls_records(&mut incoming);
                let status = match cs {
                    Err(e) => {
                        // drain before returning so the buffer is consistent
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        return Err(tls_err("tls/process", e));
                    }
                    Ok(conn_state) => {
                        let r = handle_conn_state!(
                            conn_state,
                            outgoing,
                            plaintext,
                            state.handshake_complete,
                            state
                        );
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        r
                    }
                };
                status
            }};
        }

        let status = match &mut *conn {
            TlsConnection::Client(c) => one_round!(c),
            TlsConnection::Server(s) => one_round!(s),
        };

        if let Some(kw) = status {
            return Ok(kw);
        }
        // status == None → continue looping
    }
}

// ---------------------------------------------------------------------------
// Primitive implementations
// ---------------------------------------------------------------------------

/// tls/client-state hostname [opts] → tls-state
///
/// Arity: 1–2. Signal: errors.
///
/// opts struct keys:
///   :no-verify  bool   — skip certificate verification (dev only)
///   :ca-file    string — path to PEM CA bundle
fn prim_tls_client_state(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/client-state";

    let hostname = match args[0].with_string(|s| s.to_string()) {
        Some(s) if !s.is_empty() => s,
        Some(_) => return tls_err(name, "hostname must not be empty"),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string for hostname, got {}",
                        name,
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    // Parse options struct (arg 1, optional).
    let no_verify = if args.len() > 1 {
        args[1]
            .as_struct()
            .and_then(|m| m.get(&TableKey::Keyword("no-verify".into())))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    } else {
        false
    };

    let ca_file: Option<String> = if args.len() > 1 {
        args[1]
            .as_struct()
            .and_then(|m| m.get(&TableKey::Keyword("ca-file".into())))
            .and_then(|v| v.with_string(|s| s.to_string()))
    } else {
        None
    };

    let config = match build_client_config(no_verify, ca_file.as_deref()) {
        Ok(c) => c,
        Err(e) => return tls_err(name, e),
    };

    let server_name = match rustls::pki_types::ServerName::try_from(hostname.as_str()) {
        Ok(n) => n.to_owned(),
        Err(e) => return tls_err(name, format!("invalid hostname: {}", e)),
    };

    let conn = match UnbufferedClientConnection::new(config, server_name) {
        Ok(c) => c,
        Err(e) => return tls_err(name, e),
    };

    let state = TlsState {
        conn: RefCell::new(TlsConnection::Client(conn)),
        incoming: RefCell::new(Vec::new()),
        outgoing: RefCell::new(Vec::new()),
        plaintext: RefCell::new(Vec::new()),
        handshake_complete: Cell::new(false),
        close_notify_pending: Cell::new(false),
    };

    (SIG_OK, Value::external("tls-state", state))
}

/// tls/process tls-state bytes → keyword
///
/// Arity: 2. Signal: errors.
///
/// Feeds ciphertext bytes into the TLS state machine and drives it forward.
/// Returns a status keyword: :handshaking :ready :has-data :peer-closed :closed
fn prim_tls_process(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/process";

    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // Accept bytes or @bytes (including empty).
    let new_data: Vec<u8> = if let Some(b) = args[1].as_bytes() {
        b.to_vec()
    } else if let Some(cell) = args[1].as_bytes_mut() {
        cell.borrow().to_vec()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected bytes, got {}", name, args[1].type_name()),
            ),
        );
    };

    match drive_state_machine(state, &new_data) {
        Ok(kw) => (SIG_OK, Value::keyword(kw)),
        Err(e) => e,
    }
}

/// tls/get-outgoing tls-state → bytes
///
/// Arity: 1. Signal: silent.
///
/// Drains the outgoing ciphertext buffer. Returns all bytes to send.
/// Returns empty bytes if nothing pending.
fn prim_tls_get_outgoing(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/get-outgoing";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let drained: Vec<u8> = std::mem::take(&mut *state.outgoing.borrow_mut());
    (SIG_OK, Value::bytes(drained))
}

/// tls/get-plaintext tls-state → bytes
///
/// Arity: 1. Signal: silent.
///
/// Drains the entire plaintext buffer. Returns all decrypted application data.
/// Returns empty bytes if no data.
fn prim_tls_get_plaintext(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/get-plaintext";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let drained: Vec<u8> = std::mem::take(&mut *state.plaintext.borrow_mut());
    (SIG_OK, Value::bytes(drained))
}

/// tls/read-plaintext tls-state n → bytes
///
/// Arity: 2. Signal: silent.
///
/// Drains up to n bytes from the plaintext buffer. Remainder stays buffered.
fn prim_tls_read_plaintext(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/read-plaintext";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let n = match args[1].as_int() {
        Some(i) if i >= 0 => i as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val("value-error", format!("{}: n must be non-negative", name)),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected int for n, got {}", name, args[1].type_name()),
                ),
            )
        }
    };
    let mut buf = state.plaintext.borrow_mut();
    let take = n.min(buf.len());
    let drained: Vec<u8> = buf.drain(..take).collect();
    (SIG_OK, Value::bytes(drained))
}

/// tls/plaintext-indexof tls-state byte → int or nil
///
/// Arity: 2. Signal: silent.
///
/// Scans the plaintext buffer for a byte value (0–255).
/// Returns 0-based index of first occurrence, or nil if not found.
/// Does NOT drain the buffer.
fn prim_tls_plaintext_indexof(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/plaintext-indexof";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let byte_val = match args[1].as_int() {
        Some(i) if (0..=255).contains(&i) => i as u8,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val("value-error", format!("{}: byte must be 0–255", name)),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected int for byte, got {}",
                        name,
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let buf = state.plaintext.borrow();
    match buf.iter().position(|&b| b == byte_val) {
        Some(idx) => (SIG_OK, Value::int(idx as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

/// tls/handshake-complete? tls-state → bool
///
/// Arity: 1. Signal: silent.
fn prim_tls_handshake_complete(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/handshake-complete?";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(state.handshake_complete.get()))
}

/// tls/close-notify tls-state → {:outgoing bytes}
///
/// Arity: 1. Signal: errors.
///
/// Queues a TLS close_notify alert and drives the state machine to encode it.
/// Returns {:outgoing bytes} — the encoded alert bytes to send over TCP before
/// closing the connection. Callers MUST send the outgoing bytes before calling
/// port/close on the TCP port.
///
/// Only meaningful after the handshake is complete. Calling before handshake
/// returns {:outgoing (bytes)} (empty bytes); no error is raised.
fn prim_tls_close_notify(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/close-notify";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // Set the flag so the next WriteTraffic state in the drive loop will
    // encode the alert. Then drive with empty input to trigger the transition.
    state.close_notify_pending.set(true);
    if let Err(e) = drive_state_machine(state, &[]) {
        return e;
    }

    // Drain whatever was produced (may be empty if handshake not complete).
    let outgoing: Vec<u8> = std::mem::take(&mut *state.outgoing.borrow_mut());
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("outgoing".into()), Value::bytes(outgoing));
    (SIG_OK, Value::struct_from(fields))
}

/// tls/write-plaintext tls-state plaintext → {:status :ok :outgoing bytes} or error struct
///
/// Arity: 2. Signal: errors.
///
/// Encrypts plaintext into the outgoing buffer. Only valid after handshake complete.
/// Returns {:status :ok :outgoing bytes} on success, or {:status :error :message string}.
fn prim_tls_write_plaintext(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/write-plaintext";
    let state = match get_tls_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    if !state.handshake_complete.get() {
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("status".into()), Value::keyword("error"));
        fields.insert(
            TableKey::Keyword("message".into()),
            Value::string(format!("{}: handshake not complete", name)),
        );
        return (SIG_OK, Value::struct_from(fields));
    }

    let data: Vec<u8> = if let Some(b) = args[1].as_bytes() {
        b.to_vec()
    } else if let Some(cell) = args[1].as_bytes_mut() {
        cell.borrow().to_vec()
    } else if let Some(s) = args[1].with_string(|s| s.as_bytes().to_vec()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected bytes or string, got {}",
                    name,
                    args[1].type_name()
                ),
            ),
        );
    };

    let n = data.len();
    let mut conn = state.conn.borrow_mut();
    let mut incoming = state.incoming.borrow_mut();
    let mut outgoing = state.outgoing.borrow_mut();

    // Drive the state machine to WriteTraffic, then encrypt.
    // Client and server have incompatible generic UnbufferedStatus types, so
    // we duplicate the loop body. Both branches use `return` to exit with an
    // error or fall through when continuing to loop. The only difference is
    // which connection method is called.
    //
    // Borrow discipline: ConnectionState borrows `incoming` via lifetime 'i.
    // We drain AFTER the ConnectionState is fully matched and dropped.
    loop {
        match &mut *conn {
            TlsConnection::Client(c) => {
                let UnbufferedStatus { discard, state: cs } = c.process_tls_records(&mut incoming);
                match cs {
                    Err(e) => {
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        return tls_err(name, e);
                    }
                    Ok(ConnectionState::WriteTraffic(mut wt)) => {
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        let start = outgoing.len();
                        outgoing.resize(start + n + 256, 0u8);
                        match wt.encrypt(&data, &mut outgoing[start..]) {
                            Ok(written) => {
                                outgoing.truncate(start + written);
                            }
                            Err(e) => return tls_err(name, format!("encrypt error: {}", e)),
                        }
                        break;
                    }
                    Ok(ConnectionState::EncodeTlsData(mut encode)) => {
                        let start = outgoing.len();
                        outgoing.resize(start + 16_640, 0u8);
                        let w = encode.encode(&mut outgoing[start..]);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        match w {
                            Ok(written) => {
                                outgoing.truncate(start + written);
                            }
                            Err(e) => return tls_err(name, format!("encode error: {}", e)),
                        }
                    }
                    Ok(ConnectionState::TransmitTlsData(tx)) => {
                        tx.done();
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                    }
                    Ok(ConnectionState::ReadTraffic(mut rt)) => {
                        let mut pt = state.plaintext.borrow_mut();
                        while let Some(rec) = rt.next_record() {
                            match rec {
                                Ok(app) => pt.extend_from_slice(app.payload),
                                Err(e) => {
                                    drop(pt);
                                    if discard > 0 {
                                        incoming.drain(..discard);
                                    }
                                    return tls_err(name, format!("read error: {}", e));
                                }
                            }
                        }
                        drop(pt);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                    }
                    Ok(other) => {
                        let msg = format!("{:?}", other);
                        drop(other);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        return tls_err(name, format!("unexpected state for write: {}", msg));
                    }
                }
            }
            TlsConnection::Server(s) => {
                let UnbufferedStatus { discard, state: cs } = s.process_tls_records(&mut incoming);
                match cs {
                    Err(e) => {
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        return tls_err(name, e);
                    }
                    Ok(ConnectionState::WriteTraffic(mut wt)) => {
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        let start = outgoing.len();
                        outgoing.resize(start + n + 256, 0u8);
                        match wt.encrypt(&data, &mut outgoing[start..]) {
                            Ok(written) => {
                                outgoing.truncate(start + written);
                            }
                            Err(e) => return tls_err(name, format!("encrypt error: {}", e)),
                        }
                        break;
                    }
                    Ok(ConnectionState::EncodeTlsData(mut encode)) => {
                        let start = outgoing.len();
                        outgoing.resize(start + 16_640, 0u8);
                        let w = encode.encode(&mut outgoing[start..]);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        match w {
                            Ok(written) => {
                                outgoing.truncate(start + written);
                            }
                            Err(e) => return tls_err(name, format!("encode error: {}", e)),
                        }
                    }
                    Ok(ConnectionState::TransmitTlsData(tx)) => {
                        tx.done();
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                    }
                    Ok(ConnectionState::ReadTraffic(mut rt)) => {
                        let mut pt = state.plaintext.borrow_mut();
                        while let Some(rec) = rt.next_record() {
                            match rec {
                                Ok(app) => pt.extend_from_slice(app.payload),
                                Err(e) => {
                                    drop(pt);
                                    if discard > 0 {
                                        incoming.drain(..discard);
                                    }
                                    return tls_err(name, format!("read error: {}", e));
                                }
                            }
                        }
                        drop(pt);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                    }
                    Ok(other) => {
                        let msg = format!("{:?}", other);
                        drop(other);
                        if discard > 0 {
                            incoming.drain(..discard);
                        }
                        return tls_err(name, format!("unexpected state for write: {}", msg));
                    }
                }
            }
        }
    }

    let encrypted: Vec<u8> = std::mem::take(&mut *outgoing);

    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("status".into()), Value::keyword("ok"));
    fields.insert(
        TableKey::Keyword("outgoing".into()),
        Value::bytes(encrypted),
    );
    (SIG_OK, Value::struct_from(fields))
}

/// tls/server-config cert-path key-path [opts] → tls-server-config
///
/// Arity: 2–3. Signal: errors.
fn prim_tls_server_config(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/server-config";

    let cert_path = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string for cert-path, got {}",
                        name,
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let key_path = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string for key-path, got {}",
                        name,
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    let cert_data = match std::fs::read(&cert_path) {
        Ok(d) => d,
        Err(e) => return io_err(name, format!("reading cert-path '{}': {}", cert_path, e)),
    };
    let mut cert_reader = Cursor::new(&cert_data);
    let cert_chain: Vec<rustls::pki_types::CertificateDer<'static>> =
        match certs(&mut cert_reader).collect::<Result<Vec<_>, _>>() {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => return tls_err(name, format!("no certificates found in '{}'", cert_path)),
            Err(e) => return tls_err(name, format!("cert parse error: {}", e)),
        };

    let key_data = match std::fs::read(&key_path) {
        Ok(d) => d,
        Err(e) => return io_err(name, format!("reading key-path '{}': {}", key_path, e)),
    };
    let mut key_reader = Cursor::new(&key_data);
    let private_key = match private_key(&mut key_reader) {
        Ok(Some(k)) => k,
        Ok(None) => return tls_err(name, format!("no private key found in '{}'", key_path)),
        Err(e) => return tls_err(name, format!("key parse error: {}", e)),
    };

    let config = match ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
    {
        Ok(c) => Arc::new(c),
        Err(e) => return tls_err(name, format!("server config error: {}", e)),
    };

    (
        SIG_OK,
        Value::external("tls-server-config", TlsServerConfig { config }),
    )
}

/// tls/server-state tls-server-config → tls-state
///
/// Arity: 1. Signal: errors.
fn prim_tls_server_state(args: &[Value]) -> (SignalBits, Value) {
    let name = "tls/server-state";

    let server_config = match args[0].as_external::<TlsServerConfig>() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected tls-server-config, got {}",
                        name,
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let conn = match UnbufferedServerConnection::new(Arc::clone(&server_config.config)) {
        Ok(c) => c,
        Err(e) => return tls_err(name, e),
    };

    let state = TlsState {
        conn: RefCell::new(TlsConnection::Server(conn)),
        incoming: RefCell::new(Vec::new()),
        outgoing: RefCell::new(Vec::new()),
        plaintext: RefCell::new(Vec::new()),
        handshake_complete: Cell::new(false),
        close_notify_pending: Cell::new(false),
    };

    (SIG_OK, Value::external("tls-state", state))
}

// ---------------------------------------------------------------------------
// Primitive registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "tls/client-state",
        func: prim_tls_client_state,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Create a TLS client state machine. hostname used for SNI and cert verification.\nopts: {:no-verify bool :ca-file string}",
        params: &["hostname", "opts?"],
        category: "tls",
        example: r#"(tls/client-state "example.com")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/process",
        func: prim_tls_process,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Feed ciphertext bytes into the TLS state machine.\nReturns status: :handshaking :ready :has-data :peer-closed :closed",
        params: &["tls-state", "bytes"],
        category: "tls",
        example: r#"(tls/process state (bytes))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/get-outgoing",
        func: prim_tls_get_outgoing,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Drain the outgoing ciphertext buffer. Returns bytes to send over the network.",
        params: &["tls-state"],
        category: "tls",
        example: r#"(tls/get-outgoing state)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/get-plaintext",
        func: prim_tls_get_plaintext,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Drain the entire plaintext buffer. Returns all decrypted application data.",
        params: &["tls-state"],
        category: "tls",
        example: r#"(tls/get-plaintext state)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/read-plaintext",
        func: prim_tls_read_plaintext,
        signal: Signal::silent(),
        arity: Arity::Exact(2),
        doc: "Drain up to n bytes from the plaintext buffer. Remainder stays buffered.",
        params: &["tls-state", "n"],
        category: "tls",
        example: r#"(tls/read-plaintext state 1024)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/plaintext-indexof",
        func: prim_tls_plaintext_indexof,
        signal: Signal::silent(),
        arity: Arity::Exact(2),
        doc: "Scan plaintext buffer for a byte value (0-255). Returns index or nil. Does not drain.",
        params: &["tls-state", "byte"],
        category: "tls",
        example: r#"(tls/plaintext-indexof state 10)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/handshake-complete?",
        func: prim_tls_handshake_complete,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if the TLS handshake is complete.",
        params: &["tls-state"],
        category: "tls",
        example: r#"(tls/handshake-complete? state)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/write-plaintext",
        func: prim_tls_write_plaintext,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Encrypt plaintext data. Only valid after handshake complete.\nReturns {:status :ok :outgoing bytes} or {:status :error :message string}.",
        params: &["tls-state", "data"],
        category: "tls",
        example: r#"(tls/write-plaintext state (bytes "hello"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/server-config",
        func: prim_tls_server_config,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Build a TLS server config from PEM cert and key files.",
        params: &["cert-path", "key-path", "opts?"],
        category: "tls",
        example: r#"(tls/server-config "cert.pem" "key.pem")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/server-state",
        func: prim_tls_server_state,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a TLS server state machine from a tls-server-config.",
        params: &["tls-server-config"],
        category: "tls",
        example: r#"(tls/server-state config)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tls/close-notify",
        func: prim_tls_close_notify,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Queue a TLS close_notify alert and encode it.\nReturns {:outgoing bytes} to send before closing the TCP port.",
        params: &["tls-state"],
        category: "tls",
        example: r#"(tls/close-notify state)"#,
        aliases: &[],
    },
];

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;

    fn install_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// Build a TlsState for client side with no-verify (no server needed).
    fn make_client_state() -> TlsState {
        install_provider();
        let config = build_client_config(true, None).unwrap();
        let server_name = rustls::pki_types::ServerName::try_from("localhost")
            .unwrap()
            .to_owned();
        let conn = UnbufferedClientConnection::new(config, server_name).unwrap();
        TlsState {
            conn: RefCell::new(TlsConnection::Client(conn)),
            incoming: RefCell::new(Vec::new()),
            outgoing: RefCell::new(Vec::new()),
            plaintext: RefCell::new(Vec::new()),
            handshake_complete: Cell::new(false),
            close_notify_pending: Cell::new(false),
        }
    }

    /// Wrap a TlsState in a Value::external("tls-state", ...).
    fn wrap_state(state: TlsState) -> Value {
        Value::external("tls-state", state)
    }

    // Test 1: tls/client-state with a hostname returns an ExternalObject with type_name "tls-state".
    #[test]
    fn test_client_state_creation() {
        install_provider();
        let args = vec![Value::string("example.com")];
        let (sig, val) = prim_tls_client_state(&args);
        assert_eq!(sig, SIG_OK, "prim_tls_client_state returned error");
        // Value must be an ExternalObject of type "tls-state".
        assert!(
            val.as_external::<TlsState>().is_some(),
            "returned value must be ExternalObject<TlsState>"
        );
        assert_eq!(
            val.external_type_name(),
            Some("tls-state"),
            "external type_name must be 'tls-state'"
        );
    }

    // Test 2: process with empty bytes returns handshaking status and non-empty outgoing (ClientHello).
    #[test]
    fn test_process_empty_incoming() {
        install_provider();
        let state = make_client_state();
        let state_val = wrap_state(state);

        let args = vec![state_val, Value::bytes(vec![])];
        let (sig, kw) = prim_tls_process(&args);
        assert_eq!(sig, SIG_OK, "prim_tls_process returned error");

        // Status must be :handshaking
        let kw_name = kw.as_keyword_name().expect("expected keyword result");
        assert_eq!(kw_name, "handshaking", "status must be :handshaking");

        // There must be outgoing bytes (ClientHello)
        let state_ref = args[0].as_external::<TlsState>().unwrap();
        let outgoing = state_ref.outgoing.borrow();
        assert!(
            !outgoing.is_empty(),
            "outgoing buffer must contain ClientHello bytes"
        );
    }

    // Test 3: tls/handshake-complete? returns false on fresh state.
    #[test]
    fn test_handshake_not_complete_initially() {
        install_provider();
        let state = make_client_state();
        let state_val = wrap_state(state);

        let args = vec![state_val];
        let (sig, val) = prim_tls_handshake_complete(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(
            val.as_bool(),
            Some(false),
            "handshake must not be complete on fresh state"
        );
    }

    // Test 4: after process, get-outgoing returns bytes; second call returns empty.
    #[test]
    fn test_get_outgoing_drains() {
        install_provider();
        let state = make_client_state();
        let state_val = wrap_state(state);

        // Drive the state machine to produce the ClientHello.
        let process_args = vec![state_val, Value::bytes(vec![])];
        let (sig, _) = prim_tls_process(&process_args);
        assert_eq!(sig, SIG_OK);

        // First drain: must have bytes.
        let state_val = process_args[0];
        let drain1_args = vec![state_val];
        let (sig, first) = prim_tls_get_outgoing(&drain1_args);
        assert_eq!(sig, SIG_OK);
        let first_bytes = first.as_bytes().expect("expected bytes");
        assert!(
            !first_bytes.is_empty(),
            "first get-outgoing must return ClientHello bytes"
        );

        // Second drain: must be empty.
        let (sig, second) = prim_tls_get_outgoing(&drain1_args);
        assert_eq!(sig, SIG_OK);
        let second_bytes = second.as_bytes().expect("expected bytes");
        assert!(
            second_bytes.is_empty(),
            "second get-outgoing must return empty bytes"
        );
    }

    // Test 5: tls/write-plaintext before handshake returns {:status :error}.
    #[test]
    fn test_write_plaintext_before_handshake() {
        install_provider();
        let state = make_client_state();
        let state_val = wrap_state(state);

        let args = vec![state_val, Value::bytes(b"hello".to_vec())];
        let (sig, result) = prim_tls_write_plaintext(&args);
        // Must succeed at the signal level (returns error struct, not error signal).
        assert_eq!(
            sig, SIG_OK,
            "write-plaintext must return SIG_OK (error in struct)"
        );

        let fields = result.as_struct().expect("result must be a struct");
        let status = fields
            .get(&TableKey::Keyword("status".into()))
            .expect("must have :status field");
        assert_eq!(
            status.as_keyword_name().as_deref(),
            Some("error"),
            "status must be :error when handshake not complete"
        );
    }

    // Test 6: full in-process client↔server handshake via prim_* functions.
    //
    // Uses rcgen to generate a self-signed cert and key, writes them to temp
    // files, then calls prim_tls_server_config → prim_tls_server_state and
    // prim_tls_client_state to build both sides. Drives the handshake loop
    // with prim_tls_process / prim_tls_get_outgoing, then exercises
    // prim_tls_write_plaintext for application data transfer.
    #[test]
    fn test_full_primitives_handshake() {
        install_provider();

        // Generate a self-signed cert and key via rcgen, write to temp files.
        let cert = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let tmp = std::env::temp_dir();
        let cert_path = tmp.join("elle-tls-chunk3-test.cert.pem");
        let key_path = tmp.join("elle-tls-chunk3-test.key.pem");
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.key_pair.serialize_pem()).unwrap();

        // Build server config and state via primitives.
        let cfg_args = vec![
            Value::string(cert_path.to_str().unwrap()),
            Value::string(key_path.to_str().unwrap()),
        ];
        let (sig, server_config_val) = prim_tls_server_config(&cfg_args);
        assert_eq!(sig, SIG_OK, "prim_tls_server_config failed");

        let (sig, server_state_val) = prim_tls_server_state(&[server_config_val]);
        assert_eq!(sig, SIG_OK, "prim_tls_server_state failed");

        // Build client state with :no-verify (self-signed cert).
        let mut opts_fields = BTreeMap::new();
        opts_fields.insert(TableKey::Keyword("no-verify".into()), Value::bool(true));
        let client_args = vec![Value::string("localhost"), Value::struct_from(opts_fields)];
        let (sig, client_state_val) = prim_tls_client_state(&client_args);
        assert_eq!(sig, SIG_OK, "prim_tls_client_state failed");

        // Drive the state machine for one side: feed incoming bytes, return
        // the outgoing bytes that need to be sent to the other side.
        let prim_drive = |state_val: Value, incoming: &[u8]| -> (String, Vec<u8>) {
            let incoming_val = Value::bytes(incoming.to_vec());
            let (sig, kw) = prim_tls_process(&[state_val, incoming_val]);
            assert_eq!(sig, SIG_OK, "prim_tls_process returned error");
            let status = kw
                .as_keyword_name()
                .unwrap_or_else(|| "unknown".to_string());
            let (_, out_val) = prim_tls_get_outgoing(&[state_val]);
            let outgoing = out_val.as_bytes().map(|b| b.to_vec()).unwrap_or_default();
            (status, outgoing)
        };

        // Bootstrap: feed client empty bytes to generate the ClientHello.
        let (_, mut client_to_server) = prim_drive(client_state_val, &[]);
        assert!(
            !client_to_server.is_empty(),
            "ClientHello must not be empty"
        );

        // Handshake loop: bounce bytes between client and server until both
        // sides report handshake complete. Cap at 20 iterations.
        let mut server_to_client: Vec<u8> = Vec::new();
        for _i in 0..20 {
            // Check if both sides are done.
            let (_, c_done) = prim_tls_handshake_complete(&[client_state_val]);
            let (_, s_done) = prim_tls_handshake_complete(&[server_state_val]);
            if c_done.as_bool() == Some(true) && s_done.as_bool() == Some(true) {
                break;
            }

            // Feed client → server.
            if !client_to_server.is_empty() {
                let (_, s_out) = prim_drive(server_state_val, &client_to_server);
                server_to_client = s_out;
                client_to_server = Vec::new();
            }

            // Feed server → client.
            if !server_to_client.is_empty() {
                let (_, c_out) = prim_drive(client_state_val, &server_to_client);
                client_to_server = c_out;
                server_to_client = Vec::new();
            }
        }

        // Both sides must report handshake complete.
        let (sig, c_complete) = prim_tls_handshake_complete(&[client_state_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(
            c_complete.as_bool(),
            Some(true),
            "Client handshake must be complete"
        );

        let (sig, s_complete) = prim_tls_handshake_complete(&[server_state_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(
            s_complete.as_bool(),
            Some(true),
            "Server handshake must be complete"
        );

        // Application data: client → server.
        // prim_tls_write_plaintext returns {:status :ok :outgoing bytes}.
        let plaintext_to_server = b"hello from client";
        let (sig, write_result) = prim_tls_write_plaintext(&[
            client_state_val,
            Value::bytes(plaintext_to_server.to_vec()),
        ]);
        assert_eq!(
            sig, SIG_OK,
            "prim_tls_write_plaintext (client→server) failed"
        );
        let write_fields = write_result
            .as_struct()
            .expect("write result must be a struct");
        assert_eq!(
            write_fields
                .get(&TableKey::Keyword("status".into()))
                .and_then(|v| v.as_keyword_name())
                .as_deref(),
            Some("ok"),
            "write-plaintext status must be :ok after handshake"
        );
        let ciphertext_for_server = write_fields
            .get(&TableKey::Keyword("outgoing".into()))
            .expect("write result must have :outgoing")
            .as_bytes()
            .expect(":outgoing must be bytes")
            .to_vec();
        assert!(
            !ciphertext_for_server.is_empty(),
            "encrypted client→server payload must not be empty"
        );

        // Feed ciphertext to server; then drain the decrypted plaintext.
        let _ = prim_drive(server_state_val, &ciphertext_for_server);
        let (sig, server_pt_val) = prim_tls_get_plaintext(&[server_state_val]);
        assert_eq!(sig, SIG_OK);
        let server_plaintext = server_pt_val.as_bytes().expect("plaintext must be bytes");
        assert_eq!(
            server_plaintext, plaintext_to_server,
            "Server must decrypt the exact plaintext sent by client"
        );

        // Application data: server → client.
        let plaintext_to_client = b"hello from server";
        let (sig, write_result) = prim_tls_write_plaintext(&[
            server_state_val,
            Value::bytes(plaintext_to_client.to_vec()),
        ]);
        assert_eq!(
            sig, SIG_OK,
            "prim_tls_write_plaintext (server→client) failed"
        );
        let write_fields = write_result
            .as_struct()
            .expect("write result must be a struct");
        let ciphertext_for_client = write_fields
            .get(&TableKey::Keyword("outgoing".into()))
            .expect("write result must have :outgoing")
            .as_bytes()
            .expect(":outgoing must be bytes")
            .to_vec();
        assert!(
            !ciphertext_for_client.is_empty(),
            "encrypted server→client payload must not be empty"
        );

        // Feed ciphertext to client; then drain the decrypted plaintext.
        let _ = prim_drive(client_state_val, &ciphertext_for_client);
        let (sig, client_pt_val) = prim_tls_get_plaintext(&[client_state_val]);
        assert_eq!(sig, SIG_OK);
        let client_plaintext = client_pt_val.as_bytes().expect("plaintext must be bytes");
        assert_eq!(
            client_plaintext, plaintext_to_client,
            "Client must decrypt the exact plaintext sent by server"
        );
    }
}
