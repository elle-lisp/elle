//! Elle MQTT plugin — MQTT packet codec via the `mqttbytes` crate.
//!
//! State-machine pattern: this plugin handles MQTT packet encode/decode only.
//! All TCP I/O happens in Elle code via `port/read`/`port/write`.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::error_val;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{TableKey, Value};
use mqttbytes::v4::{self, Packet};
use mqttbytes::QoS;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};

// ---------------------------------------------------------------------------
// State struct
// ---------------------------------------------------------------------------

/// MQTT protocol state machine. Type name: `"mqtt-state"`.
///
/// Holds the protocol version, packet ID counter, incoming byte buffer,
/// and parsed packet queue. No I/O happens here.
pub struct MqttState {
    /// Protocol version: 4 = MQTT 3.1.1, 5 = MQTT 5.0 (only 4 supported currently)
    #[allow(dead_code)]
    protocol: Cell<u8>,
    /// Keep-alive interval in seconds
    keep_alive: Cell<u16>,
    /// Monotonically increasing packet ID counter
    next_packet_id: Cell<u16>,
    /// Incoming raw TCP bytes not yet parsed
    incoming: RefCell<bytes::BytesMut>,
    /// Parsed packets waiting to be consumed
    packets: RefCell<VecDeque<Packet>>,
    /// True after a successful CONNACK is received
    connected: Cell<bool>,
}

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short = def.name.strip_prefix("mqtt/").unwrap_or(def.name);
        fields.insert(TableKey::Keyword(short.into()), Value::native_fn(def.func));
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_state<'a>(
    args: &'a [Value],
    idx: usize,
    name: &str,
) -> Result<&'a MqttState, (SignalBits, Value)> {
    args[idx].as_external::<MqttState>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected mqtt-state, got {}",
                    name,
                    args[idx].type_name()
                ),
            ),
        )
    })
}

fn mqtt_err(name: &str, msg: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("mqtt-error", format!("{}: {}", name, msg)),
    )
}

fn qos_from_int(n: i64) -> Option<QoS> {
    match n {
        0 => Some(QoS::AtMostOnce),
        1 => Some(QoS::AtLeastOnce),
        2 => Some(QoS::ExactlyOnce),
        _ => None,
    }
}

fn qos_to_int(q: QoS) -> i64 {
    match q {
        QoS::AtMostOnce => 0,
        QoS::AtLeastOnce => 1,
        QoS::ExactlyOnce => 2,
    }
}

/// Encode a packet into bytes using a temporary buffer.
fn encode_packet(packet: &Packet) -> Result<Vec<u8>, String> {
    let mut buf = Vec::with_capacity(256);
    match packet {
        Packet::Connect(p) => {
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::Publish(p) => {
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::Subscribe(p) => {
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::Unsubscribe(p) => {
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::PingReq => {
            let p = v4::PingReq;
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::Disconnect => {
            let p = v4::Disconnect;
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        Packet::PubAck(p) => {
            let mut b = bytes::BytesMut::new();
            p.write(&mut b).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&b);
        }
        _ => return Err("unsupported packet type for encoding".to_string()),
    }
    Ok(buf)
}

/// Convert a parsed MQTT packet to an Elle struct value.
fn packet_to_value(packet: &Packet) -> Value {
    let mut fields = BTreeMap::new();
    match packet {
        Packet::ConnAck(p) => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("connack"));
            fields.insert(
                TableKey::Keyword("session-present".into()),
                Value::bool(p.session_present),
            );
            fields.insert(
                TableKey::Keyword("code".into()),
                Value::int(match p.code {
                    v4::ConnectReturnCode::Success => 0,
                    v4::ConnectReturnCode::RefusedProtocolVersion => 1,
                    v4::ConnectReturnCode::BadClientId => 2,
                    v4::ConnectReturnCode::ServiceUnavailable => 3,
                    v4::ConnectReturnCode::BadUserNamePassword => 4,
                    v4::ConnectReturnCode::NotAuthorized => 5,
                }),
            );
        }
        Packet::Publish(p) => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("publish"));
            fields.insert(
                TableKey::Keyword("topic".into()),
                Value::string(p.topic.as_str()),
            );
            fields.insert(
                TableKey::Keyword("payload".into()),
                Value::bytes(p.payload.to_vec()),
            );
            fields.insert(
                TableKey::Keyword("qos".into()),
                Value::int(qos_to_int(p.qos)),
            );
            fields.insert(TableKey::Keyword("retain".into()), Value::bool(p.retain));
            fields.insert(
                TableKey::Keyword("packet-id".into()),
                match p.pkid {
                    0 => Value::NIL,
                    id => Value::int(id as i64),
                },
            );
        }
        Packet::SubAck(p) => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("suback"));
            fields.insert(
                TableKey::Keyword("packet-id".into()),
                Value::int(p.pkid as i64),
            );
            let codes: Vec<Value> = p
                .return_codes
                .iter()
                .map(|c| match c {
                    v4::SubscribeReasonCode::Success(qos) => Value::int(qos_to_int(*qos)),
                    v4::SubscribeReasonCode::Failure => Value::int(128),
                })
                .collect();
            fields.insert(TableKey::Keyword("codes".into()), Value::array(codes));
        }
        Packet::UnsubAck(p) => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("unsuback"));
            fields.insert(
                TableKey::Keyword("packet-id".into()),
                Value::int(p.pkid as i64),
            );
        }
        Packet::PubAck(p) => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("puback"));
            fields.insert(
                TableKey::Keyword("packet-id".into()),
                Value::int(p.pkid as i64),
            );
        }
        Packet::PingResp => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("pingresp"));
        }
        _ => {
            fields.insert(TableKey::Keyword("type".into()), Value::keyword("unknown"));
        }
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_mqtt_state(args: &[Value]) -> (SignalBits, Value) {
    let mut protocol = 4u8;
    let mut keep_alive = 60u16;

    if !args.is_empty() {
        if let Some(opts) = args[0].as_struct() {
            if let Some(v) = opts.get(&TableKey::Keyword("protocol".into())) {
                if let Some(i) = v.as_int() {
                    if i == 4 || i == 5 {
                        protocol = i as u8;
                    } else {
                        return mqtt_err("mqtt/state", "protocol must be 4 or 5");
                    }
                }
            }
            if let Some(v) = opts.get(&TableKey::Keyword("keep-alive".into())) {
                if let Some(i) = v.as_int() {
                    keep_alive = i as u16;
                }
            }
        }
    }

    let state = MqttState {
        protocol: Cell::new(protocol),
        keep_alive: Cell::new(keep_alive),
        next_packet_id: Cell::new(1),
        incoming: RefCell::new(bytes::BytesMut::new()),
        packets: RefCell::new(VecDeque::new()),
        connected: Cell::new(false),
    };
    (SIG_OK, Value::external("mqtt-state", state))
}

fn prim_mqtt_encode_connect(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-connect";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let opts = match args[1].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("{}: expected struct for opts", name)),
            )
        }
    };

    let client_id = opts
        .get(&TableKey::Keyword("client-id".into()))
        .and_then(|v| v.with_string(|s| s.to_string()))
        .unwrap_or_default();

    let clean_session = opts
        .get(&TableKey::Keyword("clean-session".into()))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut connect = v4::Connect::new(&client_id);
    connect.keep_alive = st.keep_alive.get();
    connect.clean_session = clean_session;

    if let Some(v) = opts.get(&TableKey::Keyword("username".into())) {
        if let Some(u) = v.with_string(|s| s.to_string()) {
            connect.login = Some(v4::Login {
                username: u,
                password: opts
                    .get(&TableKey::Keyword("password".into()))
                    .and_then(|v| v.with_string(|s| s.to_string()))
                    .unwrap_or_default(),
            });
        }
    }

    let packet = Packet::Connect(connect);
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_publish(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-publish";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let topic = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("{}: expected string for topic", name)),
            )
        }
    };

    let payload: Vec<u8> = if let Some(b) = args[2].as_bytes() {
        b.to_vec()
    } else if let Some(cell) = args[2].as_bytes_mut() {
        cell.borrow().to_vec()
    } else if let Some(s) = args[2].with_string(|s| s.as_bytes().to_vec()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected bytes or string for payload", name),
            ),
        );
    };

    let mut qos = QoS::AtMostOnce;
    let mut retain = false;

    if args.len() > 3 {
        if let Some(opts) = args[3].as_struct() {
            if let Some(v) = opts.get(&TableKey::Keyword("qos".into())) {
                if let Some(i) = v.as_int() {
                    qos = match qos_from_int(i) {
                        Some(q) => q,
                        None => return mqtt_err(name, format!("invalid QoS level: {}", i)),
                    };
                }
            }
            if let Some(v) = opts.get(&TableKey::Keyword("retain".into())) {
                if let Some(b) = v.as_bool() {
                    retain = b;
                }
            }
        }
    }

    let pkid = if qos != QoS::AtMostOnce {
        let id = st.next_packet_id.get();
        st.next_packet_id
            .set(if id == u16::MAX { 1 } else { id + 1 });
        id
    } else {
        0
    };

    let mut publish = v4::Publish::new(&topic, qos, payload);
    publish.pkid = pkid;
    publish.retain = retain;

    let packet = Packet::Publish(publish);
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_subscribe(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-subscribe";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // args[1] is an array of [topic qos] pairs
    let topics_val = if let Some(elems) = args[1].as_array() {
        elems.to_vec()
    } else if let Some(arr) = args[1].as_array_mut() {
        arr.borrow().to_vec()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected array of [topic qos] pairs", name),
            ),
        );
    };

    let pkid = st.next_packet_id.get();
    st.next_packet_id
        .set(if pkid == u16::MAX { 1 } else { pkid + 1 });

    let mut subscribe = v4::Subscribe::new("", QoS::AtMostOnce); // placeholder
    subscribe.pkid = pkid;
    subscribe.filters.clear();

    for item in &topics_val {
        let pair = if let Some(elems) = item.as_array() {
            elems.to_vec()
        } else if let Some(arr) = item.as_array_mut() {
            arr.borrow().to_vec()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: each topic must be [topic qos]", name),
                ),
            );
        };
        if pair.len() < 2 {
            return mqtt_err(name, "each topic must be [topic qos]");
        }
        let topic = match pair[0].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => return mqtt_err(name, "topic must be a string"),
        };
        let qos = match pair[1].as_int().and_then(qos_from_int) {
            Some(q) => q,
            None => return mqtt_err(name, "qos must be 0, 1, or 2"),
        };
        subscribe
            .filters
            .push(v4::SubscribeFilter { path: topic, qos });
    }

    let packet = Packet::Subscribe(subscribe);
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_unsubscribe(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-unsubscribe";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let topics_val = if let Some(elems) = args[1].as_array() {
        elems.to_vec()
    } else if let Some(arr) = args[1].as_array_mut() {
        arr.borrow().to_vec()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected array of topic strings", name),
            ),
        );
    };

    let pkid = st.next_packet_id.get();
    st.next_packet_id
        .set(if pkid == u16::MAX { 1 } else { pkid + 1 });

    let mut topics = Vec::with_capacity(topics_val.len());
    for item in &topics_val {
        match item.with_string(|s| s.to_string()) {
            Some(s) => topics.push(s),
            None => return mqtt_err(name, "each topic must be a string"),
        }
    }

    let mut unsub = v4::Unsubscribe::new(topics[0].clone());
    unsub.pkid = pkid;
    unsub.topics = topics;

    let packet = Packet::Unsubscribe(unsub);
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_ping(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-ping";
    let _st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let packet = Packet::PingReq;
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_disconnect(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-disconnect";
    let _st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let packet = Packet::Disconnect;
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_encode_puback(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/encode-puback";
    let _st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let pkid = match args[1].as_int() {
        Some(i) if i > 0 && i <= u16::MAX as i64 => i as u16,
        _ => return mqtt_err(name, "packet-id must be a positive integer"),
    };
    let puback = v4::PubAck::new(pkid);
    let packet = Packet::PubAck(puback);
    match encode_packet(&packet) {
        Ok(data) => (SIG_OK, Value::bytes(data)),
        Err(e) => mqtt_err(name, e),
    }
}

fn prim_mqtt_feed(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/feed";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

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

    let mut incoming = st.incoming.borrow_mut();
    incoming.extend_from_slice(&new_data);

    // Try to parse as many packets as possible from the buffer
    let mut packets = st.packets.borrow_mut();
    loop {
        match mqttbytes::v4::read(&mut incoming, 65536) {
            Ok(packet) => {
                // Track CONNACK for connected state
                if let Packet::ConnAck(ref ack) = packet {
                    if matches!(ack.code, v4::ConnectReturnCode::Success) {
                        st.connected.set(true);
                    }
                }
                packets.push_back(packet);
            }
            Err(mqttbytes::Error::InsufficientBytes(_)) => break,
            Err(e) => return mqtt_err(name, e),
        }
    }

    (SIG_OK, Value::int(packets.len() as i64))
}

fn prim_mqtt_poll(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/poll";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut packets = st.packets.borrow_mut();
    match packets.pop_front() {
        Some(packet) => (SIG_OK, packet_to_value(&packet)),
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_mqtt_poll_all(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/poll-all";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut packets = st.packets.borrow_mut();
    let vals: Vec<Value> = packets.drain(..).map(|p| packet_to_value(&p)).collect();
    (SIG_OK, Value::array(vals))
}

fn prim_mqtt_next_packet_id(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/next-packet-id";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = st.next_packet_id.get();
    st.next_packet_id
        .set(if id == u16::MAX { 1 } else { id + 1 });
    (SIG_OK, Value::int(id as i64))
}

fn prim_mqtt_connected(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/connected?";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(st.connected.get()))
}

fn prim_mqtt_keep_alive(args: &[Value]) -> (SignalBits, Value) {
    let name = "mqtt/keep-alive";
    let st = match get_state(args, 0, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(st.keep_alive.get() as i64))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "mqtt/state",
        func: prim_mqtt_state,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Create MQTT state. Optional opts: {:protocol 4 :keep-alive 60}",
        params: &["opts?"],
        category: "mqtt",
        example: r#"(mqtt/state {:protocol 4 :keep-alive 60})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-connect",
        func: prim_mqtt_encode_connect,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Encode a CONNECT packet. Returns bytes to send over TCP.",
        params: &["state", "opts"],
        category: "mqtt",
        example: r#"(mqtt/encode-connect st {:client-id "my-client"})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-publish",
        func: prim_mqtt_encode_publish,
        signal: Signal::errors(),
        arity: Arity::Range(3, 4),
        doc: "Encode a PUBLISH packet. Optional opts: {:qos 1 :retain true}",
        params: &["state", "topic", "payload", "opts?"],
        category: "mqtt",
        example: r#"(mqtt/encode-publish st "topic" "hello" {:qos 1})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-subscribe",
        func: prim_mqtt_encode_subscribe,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Encode a SUBSCRIBE packet. Topics: [[\"topic\" 0] ...]",
        params: &["state", "topics"],
        category: "mqtt",
        example: r#"(mqtt/encode-subscribe st [["sensors/#" 0]])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-unsubscribe",
        func: prim_mqtt_encode_unsubscribe,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Encode an UNSUBSCRIBE packet. Topics: [\"topic\" ...]",
        params: &["state", "topics"],
        category: "mqtt",
        example: r#"(mqtt/encode-unsubscribe st ["sensors/#"])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-ping",
        func: prim_mqtt_encode_ping,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode a PINGREQ packet.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/encode-ping st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-disconnect",
        func: prim_mqtt_encode_disconnect,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Encode a DISCONNECT packet.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/encode-disconnect st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/encode-puback",
        func: prim_mqtt_encode_puback,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Encode a PUBACK packet for a given packet ID.",
        params: &["state", "packet-id"],
        category: "mqtt",
        example: r#"(mqtt/encode-puback st 1)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/feed",
        func: prim_mqtt_feed,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Feed raw TCP bytes into the MQTT parser. Returns number of queued packets.",
        params: &["state", "bytes"],
        category: "mqtt",
        example: r#"(mqtt/feed st data)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/poll",
        func: prim_mqtt_poll,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Drain one parsed packet as a struct, or nil if none.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/poll st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/poll-all",
        func: prim_mqtt_poll_all,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Drain all parsed packets as an array.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/poll-all st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/next-packet-id",
        func: prim_mqtt_next_packet_id,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Get and increment the packet ID counter.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/next-packet-id st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/connected?",
        func: prim_mqtt_connected,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True after a successful CONNACK has been received.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/connected? st)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mqtt/keep-alive",
        func: prim_mqtt_keep_alive,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return the keep-alive interval in seconds.",
        params: &["state"],
        category: "mqtt",
        example: r#"(mqtt/keep-alive st)"#,
        aliases: &[],
    },
];
