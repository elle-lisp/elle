# elle-mqtt

Agent guide for the `elle-mqtt` plugin — MQTT packet codec via mqttbytes.

## Architecture

State-machine pattern (like TLS). The plugin handles MQTT packet encode/decode
only. All TCP I/O happens in Elle code via `port/read`/`port/write`. The
Elle-side library `lib/mqtt.lisp` drives the protocol.

## Data structures

### `MqttState` (type_name: `"mqtt-state"`)

```rust
pub struct MqttState {
    protocol: Cell<u8>,                         // 4 = v3.1.1, 5 = v5
    keep_alive: Cell<u16>,                      // seconds
    next_packet_id: Cell<u16>,                  // monotonic counter
    incoming: RefCell<bytes::BytesMut>,         // raw TCP bytes not yet parsed
    packets: RefCell<VecDeque<Packet>>,         // parsed packets waiting to be consumed
    connected: Cell<bool>,                      // true after successful CONNACK
}
```

## Primitive table

| Primitive | Arity | Signal | Returns | Purpose |
|-----------|-------|--------|---------|---------|
| `mqtt/state` | 0-1 | errors | mqtt-state | Create state, optional opts |
| `mqtt/encode-connect` | 2 | errors | bytes | Encode CONNECT packet |
| `mqtt/encode-publish` | 3-4 | errors | bytes | Encode PUBLISH packet |
| `mqtt/encode-subscribe` | 2 | errors | bytes | Encode SUBSCRIBE packet |
| `mqtt/encode-unsubscribe` | 2 | errors | bytes | Encode UNSUBSCRIBE packet |
| `mqtt/encode-ping` | 1 | errors | bytes | Encode PINGREQ packet |
| `mqtt/encode-disconnect` | 1 | errors | bytes | Encode DISCONNECT packet |
| `mqtt/encode-puback` | 2 | errors | bytes | Encode PUBACK packet |
| `mqtt/feed` | 2 | errors | int | Feed TCP bytes, return queued packet count |
| `mqtt/poll` | 1 | silent | struct or nil | Drain one parsed packet |
| `mqtt/poll-all` | 1 | silent | array | Drain all parsed packets |
| `mqtt/next-packet-id` | 1 | silent | int | Get and increment packet ID |
| `mqtt/connected?` | 1 | silent | bool | True after successful CONNACK |
| `mqtt/keep-alive` | 1 | silent | int | Keep-alive seconds |

## Packet structs returned by `mqtt/poll`

| Type | Fields |
|------|--------|
| `:connack` | `:session-present` bool, `:code` int (0=success) |
| `:publish` | `:topic` string, `:payload` bytes, `:qos` int, `:retain` bool, `:packet-id` int or nil |
| `:suback` | `:packet-id` int, `:codes` array of ints |
| `:unsuback` | `:packet-id` int |
| `:puback` | `:packet-id` int |
| `:pingresp` | (no extra fields) |

## Elle-side library

`lib/mqtt.lisp` provides the high-level API:

| Function | Purpose |
|----------|---------|
| `mqtt:connect host port opts` | TCP connect + CONNECT + wait CONNACK |
| `mqtt:publish conn topic payload opts` | PUBLISH, wait PUBACK for QoS>=1 |
| `mqtt:subscribe conn topics` | SUBSCRIBE, wait SUBACK |
| `mqtt:unsubscribe conn topics` | UNSUBSCRIBE, wait UNSUBACK |
| `mqtt:recv conn` | Read one packet from connection |
| `mqtt:listen conn callback` | Loop receiving messages |
| `mqtt:close conn` | DISCONNECT + port/close |

## Invariants

1. **No I/O in the plugin.** All network operations happen in Elle code.

2. **Buffer ownership.** `incoming` and `packets` live in the Rust struct.
   Elle feeds bytes via `mqtt/feed` and drains packets via `mqtt/poll`.

3. **Packet ID management.** The state tracks a monotonic counter. Encode
   functions for QoS>0 auto-assign IDs. `mqtt/next-packet-id` is available
   for manual use.

4. **CONNACK tracking.** `mqtt/connected?` becomes true after feeding a
   successful CONNACK through `mqtt/feed`.

## Coupling points

- `Value::external()`, `as_external::<T>()` — ExternalObject creation/access
- `error_val()` — error construction
- `PluginContext::register()` — primitive registration
- `PrimitiveDef`, `NativeFn`, `Arity`, `Signal` — primitive definition
- `SIG_OK`, `SIG_ERROR` — signal returns

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Crate definition (cdylib, mqttbytes dependency) |
| `src/lib.rs` | All primitives, state struct, entry point |
