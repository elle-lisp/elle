//! Path manipulation primitives.
//!
//! Thin wrappers around `crate::path`. No camino imports here.

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Call `f` with the string content of `val` (and the allocation ctx), or
/// return a type error tagged with `prim_name`. `f` receives `ctx` as a
/// parameter rather than capturing it, so the exclusive `&mut NativeCtx` borrow
/// passes cleanly through `with_string`'s closure instead of being aliased.
fn with_str_arg<F>(val: &Value, prim_name: &str, ctx: &mut NativeCtx, f: F) -> (SignalBits, Value)
where
    F: FnOnce(&str, &mut NativeCtx) -> (SignalBits, Value),
{
    if let Some(result) = val.with_string(|s| f(s, ctx)) {
        result
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected string, got {}", prim_name, val.type_name()),
            ),
        )
    }
}

fn prim_path_join(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        if let Some(s) = arg.with_string(|s| s.to_string()) {
            parts.push(s);
        } else {
            return type_error!(ctx, arg, "path/join", "string");
        }
    }
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    (SIG_OK, ctx.string(crate::path::join(&refs)))
}

fn prim_path_parent(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/parent",
        ctx,
        |s, ctx| match crate::path::parent(s) {
            Some(p) if !p.is_empty() => (SIG_OK, ctx.string(p)),
            Some(_) => (SIG_OK, Value::NIL), // empty parent (e.g., parent("foo") is "")
            None => (SIG_OK, Value::NIL),
        },
    )
}

fn prim_path_filename(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/filename",
        ctx,
        |s, ctx| match crate::path::filename(s) {
            Some(f) => (SIG_OK, ctx.string(f)),
            None => (SIG_OK, Value::NIL),
        },
    )
}

fn prim_path_stem(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/stem",
        ctx,
        |s, ctx| match crate::path::stem(s) {
            Some(st) => (SIG_OK, ctx.string(st)),
            None => (SIG_OK, Value::NIL),
        },
    )
}

fn prim_path_extension(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/extension",
        ctx,
        |s, ctx| match crate::path::extension(s) {
            Some(e) => (SIG_OK, ctx.string(e)),
            None => (SIG_OK, Value::NIL),
        },
    )
}

fn prim_path_with_extension(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path_str = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return type_error!(ctx, args[0], "path/with-extension", "string"),
    };
    let ext_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return type_error!(ctx, args[1], "path/with-extension", "string"),
    };
    (
        SIG_OK,
        ctx.string(crate::path::with_extension(&path_str, &ext_str)),
    )
}

fn prim_path_normalize(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/normalize", ctx, |s, ctx| {
        (SIG_OK, ctx.string(crate::path::normalize(s)))
    })
}

fn prim_path_absolute(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/absolute",
        ctx,
        |s, ctx| match crate::path::absolute(s) {
            Ok(abs) => (SIG_OK, ctx.string(abs)),
            Err(e) => crate::rich_error!(
                ctx,
                "io-error",
                format!("path/absolute: {}", e),
                path = ctx.string(s),
            ),
        },
    )
}

fn prim_path_canonicalize(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/canonicalize",
        ctx,
        |s, ctx| match crate::path::canonicalize(s) {
            Ok(c) => (SIG_OK, ctx.string(c)),
            Err(e) => crate::rich_error!(
                ctx,
                "io-error",
                format!("path/canonicalize: {}", e),
                path = ctx.string(s),
            ),
        },
    )
}

fn prim_path_relative(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path_str = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return type_error!(ctx, args[0], "path/relative", "string"),
    };
    let base_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return type_error!(ctx, args[1], "path/relative", "string"),
    };
    match crate::path::relative(&path_str, &base_str) {
        Some(rel) => (SIG_OK, ctx.string(rel)),
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_path_components(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/components", ctx, |s, ctx| {
        let parts = crate::path::components(s);
        let values: Vec<Value> = parts.into_iter().map(|s| ctx.string(s)).collect();
        (SIG_OK, ctx.list(values))
    })
}

fn prim_path_is_absolute(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/absolute?", ctx, |s, _ctx| {
        (SIG_OK, Value::bool(crate::path::is_absolute(s)))
    })
}

fn prim_path_is_relative(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/relative?", ctx, |s, _ctx| {
        (SIG_OK, Value::bool(crate::path::is_relative(s)))
    })
}

fn prim_path_cwd(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match crate::path::cwd() {
        Ok(c) => (SIG_OK, ctx.string(c)),
        Err(e) => (SIG_ERROR, ctx.error("io-error", format!("path/cwd: {}", e))),
    }
}

fn prim_path_exists(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/exists?", ctx, |s, _ctx| {
        (SIG_OK, Value::bool(crate::path::exists(s)))
    })
}

fn prim_path_is_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/file?", ctx, |s, _ctx| {
        (SIG_OK, Value::bool(crate::path::is_file(s)))
    })
}

fn prim_path_is_dir(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/dir?", ctx, |s, _ctx| {
        (SIG_OK, Value::bool(crate::path::is_dir(s)))
    })
}

primitive! {
    "path/join" => prim_path_join {
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Join path components",
        params: &["components"],
        category: "path",
        example: "(path/join \"a\" \"b\" \"c\")",
        effect: RegionEffect::Fresh,
    }
    "path/parent" => prim_path_parent {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get parent directory (nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/parent \"/home/user/data.txt\")",
        effect: RegionEffect::Fresh,
    }
    "path/filename" => prim_path_filename {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get file name (last component, nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/filename \"/home/user/data.txt\")",
        effect: RegionEffect::Fresh,
    }
    "path/stem" => prim_path_stem {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get file stem (filename without extension, nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/stem \"archive.tar.gz\")",
        effect: RegionEffect::Fresh,
    }
    "path/extension" => prim_path_extension {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get file extension without dot (nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/extension \"data.txt\")",
        effect: RegionEffect::Fresh,
    }
    "path/with-extension" => prim_path_with_extension {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Replace file extension (empty string removes it)",
        params: &["path", "ext"],
        category: "path",
        example: "(path/with-extension \"foo.txt\" \"rs\")",
        effect: RegionEffect::Fresh,
    }
    "path/normalize" => prim_path_normalize {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Lexical path normalization (resolve . and ..)",
        params: &["path"],
        category: "path",
        example: "(path/normalize \"./a/../b\")",
        effect: RegionEffect::Fresh,
    }
    "path/absolute" => prim_path_absolute {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Compute absolute path (does not require path to exist)",
        params: &["path"],
        category: "path",
        example: "(path/absolute \"src\")",
        effect: RegionEffect::Fresh,
    }
    "path/canonicalize" => prim_path_canonicalize {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Resolve path through filesystem (symlinks resolved, must exist)",
        params: &["path"],
        category: "path",
        example: "(path/canonicalize \".\")",
        effect: RegionEffect::Fresh,
    }
    "path/relative" => prim_path_relative {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compute relative path from base to target (nil if impossible)",
        params: &["target", "base"],
        category: "path",
        example: "(path/relative \"/foo/bar/baz\" \"/foo/bar\")",
        effect: RegionEffect::Fresh,
    }
    "path/components" => prim_path_components {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Split path into list of components",
        params: &["path"],
        category: "path",
        example: "(path/components \"/a/b/c\")",
        effect: RegionEffect::Fresh,
    }
    "path/absolute?" => prim_path_is_absolute {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if path is absolute",
        params: &["path"],
        category: "path",
        example: "(path/absolute? \"/foo\")",
        effect: RegionEffect::Immediate,
    }
    "path/relative?" => prim_path_is_relative {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if path is relative",
        params: &["path"],
        category: "path",
        example: "(path/relative? \"foo\")",
        effect: RegionEffect::Immediate,
    }
    "path/cwd" => prim_path_cwd {
        signal: Signal::errors(),
        doc: "Get current working directory",
        category: "path",
        example: "(path/cwd)",
        effect: RegionEffect::Fresh,
    }
    "path/exists?" => prim_path_exists {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if path exists",
        params: &["path"],
        category: "path",
        example: "(path/exists? \"data.txt\")",
        aliases: &["file-exists?", "file/exists?"],
        effect: RegionEffect::Immediate,
    }
    "path/file?" => prim_path_is_file {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if path is a regular file",
        params: &["path"],
        category: "path",
        example: "(path/file? \"data.txt\")",
        aliases: &["file?", "file/file?"],
        effect: RegionEffect::Immediate,
    }
    "path/dir?" => prim_path_is_dir {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if path is a directory",
        params: &["path"],
        category: "path",
        example: "(path/dir? \"/home\")",
        aliases: &["directory?", "file/directory?"],
        effect: RegionEffect::Immediate,
    }
}
