# elle-tls

Agent guide for the `elle-tls` plugin — TLS state machine primitives via rustls.

## Architecture

The plugin exposes rustls's `UnbufferedClientConnection` /
`UnbufferedServerConnection` as opaque `ExternalObject` values. These are
pure state machines — no I/O happens in the plugin. All socket I/O is done
in Elle code using `port/read` and `port/write` on native TCP ports.

This is the same pattern as `lib/dns.lisp` — a multi-step protocol driven
entirely in Elle, with native I/O primitives handling the network.

## Data structures

### `TlsState` (type_name: `"tls-state"`)

```rust
pub struct TlsState {
    conn: RefCell<TlsConnection>,          // Client or Server UnbufferedConnection
    incoming: RefCell<Vec<u8>>,            // ciphertext from network, not yet processed
    outgoing: RefCell<Vec<u8>>,            // ciphertext ready to send to network
    plaintext: RefCell<Vec<u8>>,           // decrypted app data, not yet consumed
    handshake_complete: Cell<bool>,
    close_notify_pending: Cell<bool>,      // set by tls/close-notify, cleared by drive loop
}
```

### `TlsServerConfig` (type_name: `"tls-server-config"`)

```rust
pub struct TlsServerConfig {
    config: Arc<rustls::ServerConfig>,
}
```

## Primitive table

| Primitive | Arity | Signal | Returns | Purpose |
|-----------|-------|--------|---------|---------|
| `tls/client-state` | 1-2 | errors | tls-state | Create client state machine |
| `tls/server-config` | 2-3 | errors | tls-server-config | Build server config from PEM files |
| `tls/server-state` | 1 | errors | tls-state | Create server state machine |
| `tls/process` | 2 | errors | keyword | Feed bytes, return status keyword |
| `tls/write-plaintext` | 2 | errors | `{:status :ok/:error :outgoing bytes}` | Encrypt plaintext after handshake |
| `tls/get-outgoing` | 1 | silent | bytes | Drain outgoing ciphertext buffer |
| `tls/get-plaintext` | 1 | silent | bytes | Drain entire plaintext buffer |
| `tls/read-plaintext` | 2 | silent | bytes | Drain up to N bytes from plaintext buffer |
| `tls/plaintext-indexof` | 2 | silent | int or nil | Scan for byte without draining |
| `tls/handshake-complete?` | 1 | silent | bool | Check handshake status |
| `tls/close-notify` | 1 | errors | `{:outgoing bytes}` | Encode close_notify alert bytes |

## Status keywords from `tls/process`

| Keyword | Meaning | Action |
|---------|---------|--------|
| `:handshaking` | Need more network data | Send outgoing, read more |
| `:ready` | Handshake just completed | Send/receive app data |
| `:has-data` | App data decrypted | Drain via `tls/get-plaintext` |
| `:peer-closed` | Peer sent close_notify | Close connection |
| `:closed` | Connection fully closed | Nothing to do |

## Error table

| Condition | Error kind | Message prefix |
|-----------|-----------|----------------|
| Bad hostname for SNI | `:tls-error` | `"tls/client-state: invalid hostname: ..."` |
| Empty hostname | `:tls-error` | `"tls/client-state: hostname must not be empty"` |
| System CA load failure | `:tls-error` | `"tls/client-state: ..."` |
| rustls protocol error | `:tls-error` | `"tls/process: ..."` |
| Write before handshake | — (returns `{:status :error :message string}`) | `"tls/write-plaintext: handshake not complete"` |
| Cert file not found | `:io-error` | `"tls/server-config: reading cert-path '...'..."` |
| Key file not found | `:io-error` | `"tls/server-config: reading key-path '...'..."` |
| No certs in PEM file | `:tls-error` | `"tls/server-config: no certificates found in '...'"` |
| Cert/key mismatch | `:tls-error` | `"tls/server-config: server config error: ..."` |
| Wrong type for arg | `:type-error` | `"tls/XXX: expected YYY, got ZZZ"` |

## Invariants

1. **No I/O in the plugin.** All network operations happen in Elle code.
   Plugin primitives are pure state machine operations.

2. **Buffer ownership.** `incoming`, `outgoing`, `plaintext` buffers live in
   the `TlsState` Rust struct. Elle code feeds and drains them via primitives.
   Elle never holds a direct reference to these buffers.

3. **Outgoing data invariant.** After every `tls/process` call, the caller
   MUST drain and send any outgoing bytes via `tls/get-outgoing` and
   `port/write`. TLS 1.3 may produce post-handshake messages at any time.
   Failing to send them will stall the connection. `tls/write-plaintext`
   returns outgoing bytes directly in its result struct — no separate drain
   needed for writes.

4. **Handshake-before-write.** `tls/write-plaintext` returns
   `{:status :error :message "tls/write-plaintext: handshake not complete"}`
   if called before `tls/handshake-complete?` returns true.

5. **close_notify must be sent before closing TCP.** Call `tls/close-notify`
   to get the encoded alert bytes, send them via `port/write`, then call
   `port/close` on the TCP port. `lib/tls.lisp`'s `tls/close` does this
   automatically.

6. **Crypto provider is global.** `ring::default_provider().install_default()`
   is called in `elle_plugin_init`. The second call (if the plugin is loaded
   twice, which cannot happen) returns `Err` which is ignored.

7. **Server config is immutable after creation.** `TlsServerConfig` wraps
   `Arc<ServerConfig>`. Multiple `tls/server-state` calls clone the Arc cheaply.

## Coupling points

- `Value::external()`, `as_external::<T>()` — ExternalObject creation/access
- `error_val()` — error construction
- `PluginContext::register()` — primitive registration
- `PrimitiveDef`, `NativeFn`, `Arity`, `Signal` — primitive definition
- `SIG_OK`, `SIG_ERROR` — signal returns

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Crate definition (cdylib, dependencies) |
| `src/lib.rs` | All primitives, structs, entry point |
