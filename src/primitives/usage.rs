// audited: 2026-11-22
//! What a process has cost: `sys/rusage` over this one, and the struct it and `subprocess/rusage` answer.
//!
//! docs/analysis/debugging.md
//! docs/subprocess.md

use crate::io::request::Usage;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;

/// `usage` as the struct both rusage primitives answer:
/// `{:user-us :sys-us :max-rss-kb}`, born in the call's own region.
pub(crate) fn usage_struct(ctx: &mut NativeCtx<'_>, usage: Usage) -> Value {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(TableKey::keyword("user-us"), Value::int(usage.user_us));
    fields.insert(TableKey::keyword("sys-us"), Value::int(usage.sys_us));
    fields.insert(
        TableKey::keyword("max-rss-kb"),
        Value::int(usage.max_rss_kb),
    );
    ctx.struct_from(fields)
}

/// What this process has cost so far: every thread it runs, terminated ones
/// included, and none of its children.
///
/// (sys/rusage) → {:user-us :sys-us :max-rss-kb}
pub(crate) fn prim_sys_rusage(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    // SAFETY: an all-zero rusage is a valid out-buffer, live for the call.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        let err = std::io::Error::last_os_error();
        return (
            SIG_ERROR,
            ctx.error("io-error", format!("sys/rusage: {err}")),
        );
    }
    (SIG_OK, usage_struct(ctx, Usage::from_rusage(&ru)))
}

primitive! {
    "sys/rusage" => prim_sys_rusage {
        signal: Signal::ambient_errors(),
        arity: Arity::Exact(0),
        doc: "Return what this process has cost so far as {:user-us :sys-us :max-rss-kb}: CPU time in user mode and in the kernel, in microseconds, summed over every thread, and the peak resident set in KiB. Counts none of the process's children.",
        params: &[],
        category: "sys",
        example: "(sys/rusage)",
        effect: RegionEffect::Fresh,
    }
}
