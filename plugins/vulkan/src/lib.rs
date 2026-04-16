mod context;
mod decode;
mod dispatch;
mod shader;

use context::GpuCtx;
use dispatch::{BufferSpec, BufferUsage, DispatchBuffer, GpuBuffer, GpuHandle};
use shader::GpuShader;

use elle::io::request::IoRequest;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

// ── Helpers ─────────────────────────────────────────────────────

fn get_ctx<'a>(val: &'a Value, name: &str) -> Result<&'a GpuCtx, (SignalBits, Value)> {
    val.as_external::<GpuCtx>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{name}: expected vulkan-ctx, got {}", val.type_name()),
            ),
        )
    })
}

fn get_shader<'a>(val: &'a Value, name: &str) -> Result<&'a GpuShader, (SignalBits, Value)> {
    val.as_external::<GpuShader>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{name}: expected vulkan-shader, got {}", val.type_name()),
            ),
        )
    })
}

fn extract_keyword(val: &Value) -> Option<String> {
    val.as_keyword_name()
}

// ── vulkan/init ─────────────────────────────────────────────────

fn prim_init(_args: &[Value]) -> (SignalBits, Value) {
    match context::init_vulkan() {
        Ok(ctx) => (SIG_OK, Value::external("vulkan-ctx", ctx)),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── vulkan/shader ───────────────────────────────────────────────

fn prim_shader(args: &[Value]) -> (SignalBits, Value) {
    let ctx = match get_ctx(&args[0], "vulkan/shader") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let num_buffers = match args[2].as_int() {
        Some(n) if n > 0 => n as u32,
        _ => {
            return (
                SIG_ERROR,
                error_val("value-error", "vulkan/shader: num-buffers must be positive"),
            )
        }
    };

    // Accept either bytes (raw SPIR-V) or string (file path)
    let spirv = if let Some(b) = args[1].as_bytes() {
        b.to_vec()
    } else if let Some(r) = args[1].as_bytes_mut() {
        r.borrow().clone()
    } else if let Some(path) = args[1].with_string(|s| s.to_string()) {
        match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("io-error", format!("vulkan/shader: {e}")),
                )
            }
        }
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "vulkan/shader: expected bytes or path string, got {}",
                    args[1].type_name()
                ),
            ),
        );
    };

    match shader::create_shader(&ctx.inner, &spirv, num_buffers) {
        Ok(s) => (SIG_OK, Value::external("vulkan-shader", s)),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── vulkan/dispatch ─────────────────────────────────────────────
// Submit GPU work, return a handle. Does NOT block.

fn prim_dispatch(args: &[Value]) -> (SignalBits, Value) {
    let shader = match get_shader(&args[0], "vulkan/dispatch") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let wg = |i: usize, name: &str| -> Result<u32, (SignalBits, Value)> {
        match args[i].as_int() {
            Some(n) if n > 0 => Ok(n as u32),
            _ => Err((
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("vulkan/dispatch: {name} must be positive"),
                ),
            )),
        }
    };
    let wg_x = match wg(1, "wg-x") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let wg_y = match wg(2, "wg-y") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let wg_z = match wg(3, "wg-z") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let buf_specs_arr = if let Some(a) = args[4].as_array() {
        a.to_vec()
    } else if let Some(r) = args[4].as_array_mut() {
        r.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", "vulkan/dispatch: buffers must be an array"),
        );
    };

    let mut dbufs = Vec::with_capacity(buf_specs_arr.len());
    for (i, spec_val) in buf_specs_arr.iter().enumerate() {
        match parse_dispatch_buffer(spec_val, i, "vulkan/dispatch") {
            Ok(db) => dbufs.push(db),
            Err(e) => return e,
        }
    }

    if dbufs.len() != shader.num_buffers as usize {
        return (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "vulkan/dispatch: shader expects {} buffers, got {}",
                    shader.num_buffers,
                    dbufs.len()
                ),
            ),
        );
    }

    match dispatch::dispatch(
        shader.ctx.clone(),
        shader.pipeline,
        shader.pipeline_layout,
        shader.descriptor_set_layout,
        [wg_x, wg_y, wg_z],
        dbufs,
    ) {
        Ok(handle) => (SIG_OK, Value::external("vulkan-handle", handle)),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── vulkan/wait ─────────────────────────────────────────────────
// Yield on the fence fd. Fiber suspends until GPU completes.
// No thread pool thread consumed.

fn prim_wait(args: &[Value]) -> (SignalBits, Value) {
    let handle = match args[0].as_external::<GpuHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "vulkan/wait: expected vulkan-handle, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let fd = handle.fence_fd;
    (
        SIG_YIELD | SIG_IO,
        IoRequest::poll_fd(fd, libc::POLLIN as u32),
    )
}

// ── vulkan/collect ──────────────────────────────────────────────
// Read back results after GPU completes. Returns bytes.

fn prim_collect(args: &[Value]) -> (SignalBits, Value) {
    // Take ownership — GpuHandle is behind Rc, we need to extract it.
    // Since as_external returns &T (behind Rc), we can't move out.
    // Instead, we'll do readback while the handle is borrowed, then
    // let Drop handle cleanup.
    let handle = match args[0].as_external::<GpuHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "vulkan/collect: expected vulkan-handle, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    match dispatch::collect_ref(handle) {
        Ok(bytes) => (SIG_OK, Value::bytes(bytes)),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── vulkan/submit (convenience: dispatch + wait + collect) ──────

fn prim_submit(args: &[Value]) -> (SignalBits, Value) {
    // For backward compat and the common case: dispatch, block on
    // thread pool, return results. Uses IoOp::Task.
    let shader = match get_shader(&args[0], "vulkan/submit") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let wg = |i: usize, name: &str| -> Result<u32, (SignalBits, Value)> {
        match args[i].as_int() {
            Some(n) if n > 0 => Ok(n as u32),
            _ => Err((
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("vulkan/submit: {name} must be positive"),
                ),
            )),
        }
    };
    let wg_x = match wg(1, "wg-x") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let wg_y = match wg(2, "wg-y") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let wg_z = match wg(3, "wg-z") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let buf_specs_arr = if let Some(a) = args[4].as_array() {
        a.to_vec()
    } else if let Some(r) = args[4].as_array_mut() {
        r.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", "vulkan/submit: buffers must be an array"),
        );
    };

    let mut dbufs = Vec::with_capacity(buf_specs_arr.len());
    for (i, spec_val) in buf_specs_arr.iter().enumerate() {
        match parse_dispatch_buffer(spec_val, i, "vulkan/submit") {
            Ok(db) => dbufs.push(db),
            Err(e) => return e,
        }
    }

    if dbufs.len() != shader.num_buffers as usize {
        return (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "vulkan/submit: shader expects {} buffers, got {}",
                    shader.num_buffers,
                    dbufs.len()
                ),
            ),
        );
    }

    let ctx_arc = shader.ctx.clone();
    let pipeline = shader.pipeline;
    let pipeline_layout = shader.pipeline_layout;
    let descriptor_set_layout = shader.descriptor_set_layout;

    let task = move || -> (i32, Vec<u8>) {
        match dispatch::dispatch(
            ctx_arc.clone(),
            pipeline,
            pipeline_layout,
            descriptor_set_layout,
            [wg_x, wg_y, wg_z],
            dbufs,
        ) {
            Ok(handle) => {
                // Block on fence (we're on thread pool, this is fine)
                let state = ctx_arc.lock().unwrap();
                unsafe {
                    state
                        .device
                        .wait_for_fences(&[handle.fence], true, u64::MAX)
                }
                .ok();
                drop(state);
                match dispatch::collect_ref(&handle) {
                    Ok(bytes) => (0, bytes),
                    Err(msg) => (-1, msg.into_bytes()),
                }
            }
            Err(msg) => (-1, msg.into_bytes()),
        }
    };

    (SIG_YIELD | SIG_IO, IoRequest::task(task))
}

/// Parse a dispatch buffer: either a persistent GpuBuffer or a fresh BufferSpec.
fn parse_dispatch_buffer(
    val: &Value,
    index: usize,
    caller: &str,
) -> Result<DispatchBuffer, (SignalBits, Value)> {
    // Check if it's a persistent GpuBuffer
    if let Some(gpu_buf) = val.as_external::<GpuBuffer>() {
        return Ok(DispatchBuffer::Persistent {
            buffer: gpu_buf.buffer,
            byte_size: gpu_buf.byte_size,
            usage: BufferUsage::Input, // persistent buffers are input-only for now
        });
    }
    // Fall back to parsing as a buffer spec
    parse_buffer_spec(val, index, caller).map(DispatchBuffer::Spec)
}

fn parse_buffer_spec(
    val: &Value,
    index: usize,
    caller: &str,
) -> Result<BufferSpec, (SignalBits, Value)> {
    let err =
        |kind: &str, msg: String| -> (SignalBits, Value) { (SIG_ERROR, error_val(kind, msg)) };

    let usage_val = struct_get(val, "usage").ok_or_else(|| {
        err(
            "value-error",
            format!("{caller}: buffer[{index}] missing :usage"),
        )
    })?;

    let usage = match extract_keyword(&usage_val) {
        Some(ref k) if k == "input" => BufferUsage::Input,
        Some(ref k) if k == "output" => BufferUsage::Output,
        Some(ref k) if k == "inout" => BufferUsage::InOut,
        _ => {
            return Err(err(
                "value-error",
                format!("{caller}: buffer[{index}] :usage must be :input, :output, or :inout"),
            ))
        }
    };

    if usage == BufferUsage::Output {
        let size_val = struct_get(val, "size").ok_or_else(|| {
            err(
                "value-error",
                format!("{caller}: output buffer[{index}] missing :size"),
            )
        })?;
        let byte_size = size_val.as_int().ok_or_else(|| {
            err(
                "type-error",
                format!("{caller}: buffer[{index}] :size must be integer"),
            )
        })? as usize;
        return Ok(BufferSpec {
            data: Vec::new(),
            byte_size,
            usage,
        });
    }

    // Input or InOut: needs :data (array of numeric values)
    let data_val = struct_get(val, "data").ok_or_else(|| {
        err(
            "value-error",
            format!("{caller}: buffer[{index}] missing :data"),
        )
    })?;

    let arr = if let Some(a) = data_val.as_array() {
        a.to_vec()
    } else if let Some(r) = data_val.as_array_mut() {
        r.borrow().clone()
    } else {
        return Err(err(
            "type-error",
            format!("{caller}: buffer[{index}] :data must be an array"),
        ));
    };

    // Encode to raw bytes. Default dtype is :f32; also supports :u32, :i32, :i64.
    let dtype = struct_get(val, "dtype")
        .and_then(|v| extract_keyword(&v))
        .unwrap_or_else(|| "f32".to_string());

    let elem_size = if dtype == "i64" { 8 } else { 4 };
    let mut bytes = Vec::with_capacity(arr.len() * elem_size);
    for (j, v) in arr.iter().enumerate() {
        match dtype.as_str() {
            "f32" => {
                let f = if let Some(f) = v.as_float() { f as f32 }
                    else if let Some(i) = v.as_int() { i as f32 }
                    else {
                        return Err(err("type-error",
                            format!("{caller}: buffer[{index}][{j}] must be numeric, got {}", v.type_name())));
                    };
                bytes.extend_from_slice(&f.to_le_bytes());
            }
            "u32" => {
                let n = v.as_int()
                    .ok_or_else(|| err("type-error",
                        format!("{caller}: buffer[{index}][{j}] must be integer for :u32")))?;
                bytes.extend_from_slice(&(n as u32).to_le_bytes());
            }
            "i32" => {
                let n = v.as_int()
                    .ok_or_else(|| err("type-error",
                        format!("{caller}: buffer[{index}][{j}] must be integer for :i32")))?;
                bytes.extend_from_slice(&(n as i32).to_le_bytes());
            }
            "i64" => {
                let n = v.as_int()
                    .ok_or_else(|| err("type-error",
                        format!("{caller}: buffer[{index}][{j}] must be integer for :i64")))?;
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            _ => return Err(err("value-error",
                format!("{caller}: buffer[{index}] unsupported :dtype {dtype:?}, expected :f32, :u32, :i32, or :i64"))),
        }
    }

    let byte_size = bytes.len();
    Ok(BufferSpec {
        data: bytes,
        byte_size,
        usage,
    })
}

fn struct_get(val: &Value, key: &str) -> Option<Value> {
    if let Some(fields) = val.as_struct() {
        fields
            .get(&elle::value::TableKey::Keyword(key.to_string()))
            .copied()
    } else if let Some(fields) = val.as_struct_mut() {
        fields
            .borrow()
            .get(&elle::value::TableKey::Keyword(key.to_string()))
            .copied()
    } else {
        None
    }
}

// ── vulkan/decode ───────────────────────────────────────────────

fn prim_decode(args: &[Value]) -> (SignalBits, Value) {
    let bytes = if let Some(b) = args[0].as_bytes() {
        b.to_vec()
    } else if let Some(r) = args[0].as_bytes_mut() {
        r.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("vulkan/decode: expected bytes, got {}", args[0].type_name()),
            ),
        );
    };

    let dtype = match extract_keyword(&args[1]) {
        Some(k) if matches!(k.as_str(), "f32" | "u32" | "i32" | "i64" | "raw") => k,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "vulkan/decode: dtype must be :f32, :u32, :i32, :i64, or :raw",
                ),
            )
        }
    };

    match decode::decode(&bytes, &dtype) {
        Ok(val) => (SIG_OK, val),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── vulkan/f32-bits ─────────────────────────────────────────────

fn prim_f32_bits(args: &[Value]) -> (SignalBits, Value) {
    let f = if let Some(f) = args[0].as_float() {
        f
    } else if let Some(i) = args[0].as_int() {
        i as f64
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "vulkan/f32-bits: expected number, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };
    let bits = (f as f32).to_bits();
    (SIG_OK, Value::int(bits as i64))
}

// ── vulkan/persist ──────────────────────────────────────────────
// Create a persistent GPU buffer. Uploaded once, reused across dispatches.

fn prim_persist(args: &[Value]) -> (SignalBits, Value) {
    let ctx = match get_ctx(&args[0], "vulkan/persist") {
        Ok(c) => c,
        Err(e) => return e,
    };

    let spec = match parse_buffer_spec(&args[1], 0, "vulkan/persist") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let location = match spec.usage {
        BufferUsage::Input => gpu_allocator::MemoryLocation::CpuToGpu,
        BufferUsage::Output => gpu_allocator::MemoryLocation::GpuToCpu,
        BufferUsage::InOut => gpu_allocator::MemoryLocation::CpuToGpu,
    };

    let mut state = match ctx.inner.lock() {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, error_val("gpu-error", format!("lock: {e}"))),
    };

    let (buffer, allocation) = match state.acquire_buffer(spec.byte_size, location, 0) {
        Ok(ba) => ba,
        Err(msg) => return (SIG_ERROR, error_val("gpu-error", msg)),
    };

    let gpu_buf = GpuBuffer {
        ctx: ctx.inner.clone(),
        buffer,
        allocation,
        byte_size: spec.byte_size,
        location,
    };

    // Upload initial data
    if !spec.data.is_empty() {
        if let Err(msg) = gpu_buf.upload(&spec.data) {
            return (SIG_ERROR, error_val("gpu-error", msg));
        }
    }

    (SIG_OK, Value::external("vulkan-buffer", gpu_buf))
}

// ── vulkan/update ──────────────────────────────────────────────
// Re-upload data to a persistent GPU buffer.

fn prim_update(args: &[Value]) -> (SignalBits, Value) {
    let gpu_buf = match args[0].as_external::<GpuBuffer>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "vulkan/update: expected vulkan-buffer, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let spec = match parse_buffer_spec(&args[1], 0, "vulkan/update") {
        Ok(s) => s,
        Err(e) => return e,
    };

    match gpu_buf.upload(&spec.data) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, error_val("gpu-error", msg)),
    }
}

// ── Primitive table ─────────────────────────────────────────────

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "vulkan/init",
        func: prim_init,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Initialize Vulkan GPU context",
        params: &[],
        category: "gpu",
        example: "(vulkan/init)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/shader",
        func: prim_shader,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Load SPIR-V shader from file path, create compute pipeline",
        params: &["ctx", "path", "num-buffers"],
        category: "gpu",
        example: "(vulkan/shader ctx \"shader.spv\" 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/dispatch",
        func: prim_dispatch,
        signal: Signal::errors(),
        arity: Arity::Exact(5),
        doc: "Submit GPU compute work, return handle (non-blocking)",
        params: &["shader", "wg-x", "wg-y", "wg-z", "buffers"],
        category: "gpu",
        example: "(vulkan/dispatch shader 4 1 1 bufs)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/wait",
        func: prim_wait,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Wait for GPU dispatch to complete (fiber suspends on fence fd)",
        params: &["handle"],
        category: "gpu",
        example: "(vulkan/wait handle)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/collect",
        func: prim_collect,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read back results after GPU completes",
        params: &["handle"],
        category: "gpu",
        example: "(vulkan/collect handle)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/submit",
        func: prim_submit,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Exact(5),
        doc: "Dispatch + wait + collect in one call (thread pool, convenience)",
        params: &["shader", "wg-x", "wg-y", "wg-z", "buffers"],
        category: "gpu",
        example: "(vulkan/submit shader 4 1 1 bufs)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/f32-bits",
        func: prim_f32_bits,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return IEEE 754 f32 bit pattern of a number as integer",
        params: &["number"],
        category: "gpu",
        example: "(vulkan/f32-bits 1.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/decode",
        func: prim_decode,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Decode GPU result bytes to Elle float arrays",
        params: &["result-bytes", "element-type"],
        category: "gpu",
        example: "(vulkan/decode result :f32)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/persist",
        func: prim_persist,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create a persistent GPU buffer from a buffer spec",
        params: &["ctx", "buffer-spec"],
        category: "gpu",
        example: "(vulkan/persist ctx (gpu:input data))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vulkan/update",
        func: prim_update,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Re-upload data to a persistent GPU buffer",
        params: &["gpu-buffer", "buffer-spec"],
        category: "gpu",
        example: "(vulkan/update buf (gpu:input new-data))",
        aliases: &[],
    },
];

elle::elle_plugin_init!(PRIMITIVES, "vulkan/");
