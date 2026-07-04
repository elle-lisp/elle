// h2cross — differential HTTP/2 echo-amplify test tool
//
// Protocol:
//   1. Client opens one bidi stream (POST /echo-amplify)
//   2. Client sends `count` small messages of `request-size` bytes each
//   3. Client half-closes (END_STREAM on last DATA or empty DATA+ES)
//   4. Server reads all messages, echoes back one response per message,
//      amplified to `response-size` bytes each
//   5. Client reads all responses, reports count + total bytes
//
// Hypothesis flags (composable):
//   --grpc-framing    H2: gRPC 5-byte prefix instead of 4-byte length prefix
//   --trailers        H3: END_STREAM on trailing HEADERS, not on last DATA
//   --window-size N   H4: initial window size in bytes (default: h2 crate default)

use bytes::{Buf, BufMut, Bytes, BytesMut};
use clap::{Parser, Subcommand};
use h2::server::SendResponse;
use h2::RecvStream;
use std::time::Instant;
use tokio::net::{TcpListener, TcpStream};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run as h2 echo-amplify server
    Server {
        /// Address to listen on
        #[arg(long, default_value = "127.0.0.1:0")]
        addr: String,
        /// Response size per message in bytes
        #[arg(long, default_value_t = 12000)]
        response_size: usize,
        /// H2: Use gRPC 5-byte framing instead of 4-byte length prefix
        #[arg(long)]
        grpc_framing: bool,
        /// H3: Send trailing HEADERS with grpc-status instead of END_STREAM on DATA
        #[arg(long)]
        trailers: bool,
        /// H4: Initial stream and connection window size in bytes
        #[arg(long)]
        window_size: Option<u32>,
    },
    /// Run as h2 echo-amplify client
    Client {
        /// Server address (host:port)
        #[arg(long)]
        target: String,
        /// Number of messages to send
        #[arg(long, default_value_t = 650)]
        count: usize,
        /// Size of each request message in bytes
        #[arg(long, default_value_t = 100)]
        request_size: usize,
        /// Expected response size per message (for verification)
        #[arg(long, default_value_t = 12000)]
        response_size: usize,
        /// H2: Use gRPC 5-byte framing instead of 4-byte length prefix
        #[arg(long)]
        grpc_framing: bool,
        /// H3: Expect trailing HEADERS instead of END_STREAM on last DATA
        #[arg(long)]
        trailers: bool,
        /// H10: Close connection after receiving this many messages (0 = read all)
        #[arg(long, default_value_t = 0)]
        close_after: usize,
    },
}

fn ts() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    format!("{:.6}", now.as_secs_f64())
}

/// Encode a plain length-prefixed message: [4-byte BE length][payload]
fn encode_plain(payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(4 + payload.len());
    buf.put_u32(payload.len() as u32);
    buf.put_slice(payload);
    buf.freeze()
}

/// Encode a gRPC-framed message: [0x00][4-byte BE length][payload]
fn encode_grpc(payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(5 + payload.len());
    buf.put_u8(0x00); // compressed flag = false
    buf.put_u32(payload.len() as u32);
    buf.put_slice(payload);
    buf.freeze()
}

fn encode_msg(payload: &[u8], grpc_framing: bool) -> Bytes {
    if grpc_framing {
        encode_grpc(payload)
    } else {
        encode_plain(payload)
    }
}

/// Decode plain 4-byte length-prefixed messages from buffer.
fn decode_plain(buf: &mut BytesMut) -> Vec<Bytes> {
    let mut msgs = Vec::new();
    loop {
        if buf.len() < 4 {
            break;
        }
        let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if buf.len() < 4 + len {
            break;
        }
        buf.advance(4);
        msgs.push(buf.split_to(len).freeze());
    }
    msgs
}

/// Decode gRPC 5-byte framed messages from buffer.
fn decode_grpc(buf: &mut BytesMut) -> Vec<Bytes> {
    let mut msgs = Vec::new();
    loop {
        if buf.len() < 5 {
            break;
        }
        // byte 0: compressed flag (ignored)
        let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
        if buf.len() < 5 + len {
            break;
        }
        buf.advance(5);
        msgs.push(buf.split_to(len).freeze());
    }
    msgs
}

fn decode_msgs(buf: &mut BytesMut, grpc_framing: bool) -> Vec<Bytes> {
    if grpc_framing {
        decode_grpc(buf)
    } else {
        decode_plain(buf)
    }
}

fn prefix_size(grpc_framing: bool) -> usize {
    if grpc_framing { 5 } else { 4 }
}

/// Read all DATA from a recv stream, decode messages.
/// If expect_trailers is true, also reads trailers after DATA is exhausted.
/// If close_after > 0, stops reading after that many decoded messages.
async fn read_all_messages(
    mut body: RecvStream,
    grpc_framing: bool,
    expect_trailers: bool,
    close_after: usize,
) -> Result<(usize, usize, bool), String> {
    let mut buf = BytesMut::new();
    let mut msg_count = 0usize;
    let mut total_bytes = 0usize;
    let mut frame_count = 0usize;

    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|e| format!("recv data error: {e}"))?;
        frame_count += 1;
        total_bytes += chunk.len();
        if frame_count % 64 == 0 {
            eprintln!(
                "[{}] recv DATA frame #{frame_count}, {} bytes this frame, {total_bytes} total",
                ts(),
                chunk.len()
            );
        }
        body.flow_control()
            .release_capacity(chunk.len())
            .map_err(|e| format!("release_capacity error: {e}"))?;
        buf.extend_from_slice(&chunk);
        let msgs = decode_msgs(&mut buf, grpc_framing);
        msg_count += msgs.len();
        if close_after > 0 && msg_count >= close_after {
            eprintln!(
                "[{}] close_after={close_after} reached at {msg_count} messages, stopping",
                ts()
            );
            // Drop body to close the recv side
            return Ok((msg_count, total_bytes, false));
        }
    }
    // Drain any remaining partial messages
    let msgs = decode_msgs(&mut buf, grpc_framing);
    msg_count += msgs.len();

    // Read trailers if expected
    let got_trailers = if expect_trailers {
        match body.trailers().await {
            Ok(Some(trailers)) => {
                let status = trailers
                    .get("grpc-status")
                    .map(|v| v.to_str().unwrap_or("?").to_string())
                    .unwrap_or_else(|| "missing".to_string());
                eprintln!(
                    "[{}] recv trailers: grpc-status={status}",
                    ts()
                );
                true
            }
            Ok(None) => {
                eprintln!("[{}] WARNING: expected trailers but got None", ts());
                false
            }
            Err(e) => {
                eprintln!("[{}] WARNING: trailers error: {e}", ts());
                false
            }
        }
    } else {
        false
    };

    eprintln!(
        "[{}] stream done: {msg_count} messages, {total_bytes} bytes, {frame_count} DATA frames, trailers={got_trailers}",
        ts()
    );
    Ok((msg_count, total_bytes, got_trailers))
}

// ── Server stream handler ──────────────────────────────────────────────

async fn handle_stream(
    mut send: SendResponse<Bytes>,
    recv: RecvStream,
    response_size: usize,
    grpc_framing: bool,
    trailers: bool,
) -> Result<(), String> {
    let (msg_count, req_bytes, _) = read_all_messages(recv, grpc_framing, false, 0).await?;
    let mode_str = format!(
        "grpc_framing={grpc_framing} trailers={trailers}"
    );
    eprintln!(
        "[{}] server[{mode_str}]: received {msg_count} messages ({req_bytes} bytes), \
         sending {msg_count} × {response_size} byte responses",
        ts()
    );

    // Send initial response HEADERS
    let mut builder = http::Response::builder().status(200);
    if grpc_framing {
        builder = builder.header("content-type", "application/grpc");
    }
    let response = builder.body(()).unwrap();
    let mut send_stream = send
        .send_response(response, false)
        .map_err(|e| format!("send_response error: {e}"))?;

    // Send one response per received message
    let payload = vec![0xABu8; response_size];
    for i in 0..msg_count {
        let msg = encode_msg(&payload, grpc_framing);
        // If trailers mode: never set END_STREAM on DATA
        // If plain mode: set END_STREAM on last DATA
        let end_stream = if trailers {
            false
        } else {
            i == msg_count - 1
        };
        send_data_with_flow_control(&mut send_stream, &msg, end_stream).await?;
        if (i + 1) % 64 == 0 {
            eprintln!("[{}] server: sent response {}/{msg_count}", ts(), i + 1);
        }
    }

    // H3: send trailing HEADERS with grpc-status
    if trailers {
        let mut trailer_map = http::HeaderMap::new();
        trailer_map.insert("grpc-status", "0".parse().unwrap());
        trailer_map.insert("grpc-message", "OK".parse().unwrap());
        send_stream
            .send_trailers(trailer_map)
            .map_err(|e| format!("send_trailers error: {e}"))?;
        eprintln!(
            "[{}] server: sent trailers (grpc-status: 0)",
            ts()
        );
    }

    eprintln!("[{}] server: done sending all {msg_count} responses", ts());
    Ok(())
}

/// Send data on a SendStream, respecting flow control.
async fn send_data_with_flow_control(
    send_stream: &mut h2::SendStream<Bytes>,
    msg: &[u8],
    end_stream: bool,
) -> Result<(), String> {
    send_stream.reserve_capacity(msg.len());
    loop {
        let cap = send_stream.capacity();
        if cap > 0 {
            break;
        }
        tokio::task::yield_now().await;
        send_stream.reserve_capacity(msg.len());
    }

    let mut remaining = &msg[..];
    while !remaining.is_empty() {
        let cap = send_stream.capacity();
        if cap == 0 {
            send_stream.reserve_capacity(remaining.len());
            tokio::task::yield_now().await;
            continue;
        }
        let chunk_len = cap.min(remaining.len());
        let chunk = Bytes::copy_from_slice(&remaining[..chunk_len]);
        remaining = &remaining[chunk_len..];
        let is_last = remaining.is_empty() && end_stream;
        send_stream
            .send_data(chunk, is_last)
            .map_err(|e| format!("send_data error: {e}"))?;
    }
    Ok(())
}

// ── Server ─────────────────────────────────────────────────────────────

async fn run_server(
    addr: &str,
    response_size: usize,
    grpc_framing: bool,
    trailers: bool,
    window_size: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    println!("LISTENING {}", local_addr.port());
    eprintln!(
        "[{}] server: listening on {local_addr} grpc_framing={grpc_framing} trailers={trailers} window_size={window_size:?}",
        ts()
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        eprintln!("[{}] server: accepted connection from {peer}", ts());
        let resp_size = response_size;
        let gf = grpc_framing;
        let tr = trailers;
        let ws = window_size;
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, resp_size, gf, tr, ws).await {
                eprintln!("[{}] server: connection error: {e}", ts());
            }
        });
    }
}

async fn handle_connection(
    socket: TcpStream,
    response_size: usize,
    grpc_framing: bool,
    trailers: bool,
    window_size: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = if let Some(ws) = window_size {
        let mut builder = h2::server::Builder::new();
        builder
            .initial_window_size(ws)
            .initial_connection_window_size(ws);
        builder.handshake(socket).await?
    } else {
        h2::server::handshake(socket).await?
    };

    eprintln!("[{}] server: h2 handshake complete", ts());

    while let Some(result) = connection.accept().await {
        let (request, send_response) = result?;
        eprintln!(
            "[{}] server: new stream {} {} {}",
            ts(),
            request.method(),
            request.uri(),
            request
                .headers()
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("?")))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let resp_size = response_size;
        let gf = grpc_framing;
        let tr = trailers;
        tokio::spawn(async move {
            let body = request.into_body();
            if let Err(e) = handle_stream(send_response, body, resp_size, gf, tr).await {
                eprintln!("[{}] server: stream error: {e}", ts());
            }
        });
    }

    Ok(())
}

// ── Client ─────────────────────────────────────────────────────────────

async fn run_client(
    target: &str,
    count: usize,
    request_size: usize,
    response_size: usize,
    grpc_framing: bool,
    expect_trailers: bool,
    close_after: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();

    let tcp = TcpStream::connect(target).await?;
    eprintln!(
        "[{}] client: connected to {target} grpc_framing={grpc_framing} trailers={expect_trailers}",
        ts()
    );

    let (client, h2) = h2::client::handshake(tcp).await?;
    tokio::spawn(async move {
        if let Err(e) = h2.await {
            eprintln!("[{}] client: connection error: {e}", ts());
        }
    });

    let mut client = client.ready().await?;
    eprintln!(
        "[{}] client: h2 handshake complete, sending {count} messages",
        ts()
    );

    let request = http::Request::builder()
        .method("POST")
        .uri("http://localhost/echo-amplify")
        .body(())
        .unwrap();

    let (response_future, mut send_stream) = client.send_request(request, false)?;

    // Send messages
    let payload = vec![0x42u8; request_size];
    for i in 0..count {
        let msg = encode_msg(&payload, grpc_framing);
        let end_stream = i == count - 1;
        send_data_with_flow_control(&mut send_stream, &msg, end_stream)
            .await
            .map_err(|e| format!("client send: {e}"))?;
        if (i + 1) % 64 == 0 {
            eprintln!("[{}] client: sent message {}/{count}", ts(), i + 1);
        }
    }

    eprintln!(
        "[{}] client: all {count} messages sent, reading responses",
        ts()
    );

    let response = response_future.await?;
    eprintln!(
        "[{}] client: got response status {}",
        ts(),
        response.status()
    );

    let body = response.into_body();
    let (msg_count, total_bytes, got_trailers) =
        read_all_messages(body, grpc_framing, expect_trailers, close_after).await?;

    let elapsed = start.elapsed();
    let expected_bytes = count * (prefix_size(grpc_framing) + response_size);
    let pass = if close_after > 0 {
        msg_count >= close_after
    } else {
        msg_count == count && (!expect_trailers || got_trailers)
    };

    println!(
        "RESULT count={count} request_size={request_size} response_size={response_size} \
         grpc_framing={grpc_framing} trailers={expect_trailers} close_after={close_after}"
    );
    println!("  messages_sent={count}");
    println!("  messages_received={msg_count}");
    println!("  bytes_received={total_bytes}");
    println!("  expected_bytes={expected_bytes}");
    println!("  got_trailers={got_trailers}");
    println!("  elapsed_ms={}", elapsed.as_millis());
    println!("  status={}", if pass { "PASS" } else { "FAIL" });

    if !pass {
        std::process::exit(1);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Server {
            addr,
            response_size,
            grpc_framing,
            trailers,
            window_size,
        } => run_server(&addr, response_size, grpc_framing, trailers, window_size).await?,
        Command::Client {
            target,
            count,
            request_size,
            response_size,
            grpc_framing,
            trailers,
            close_after,
        } => {
            run_client(
                &target,
                count,
                request_size,
                response_size,
                grpc_framing,
                trailers,
                close_after,
            )
            .await?
        }
    }
    Ok(())
}
