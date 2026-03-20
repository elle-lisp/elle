# Design Plan Review: elle-tls Plugin

## Executive Summary

The plan is **architecturally sound** and aligns well with Elle's design principles. The sans-I/O approach is correct and matches how rustls is designed. However, there are **critical gaps** in the plan regarding:

1. **Missing `bytes/find` primitive** — the plan depends on this for line-oriented reads
2. **Mutable bytes operations** — the plan needs efficient buffer manipulation
3. **Plugin loading mechanism** — the plan's assumption about `(load "lib/tls.lisp")` is incorrect
4. **Coroutine protocol verification** — confirmed working, but with caveats

---

## Verified Claims

### ✅ Port Validation in Stream Primitives

**Claim:** `stream/read-line`, `stream/read`, `stream/write` validate their argument is a `Port` via `as_external::<Port>()`. TLS-conn cannot be passed to these.

**Evidence:**
- `src/primitives/stream.rs:17-28` — `extract_port_value()` checks `value.as_external::<Port>().is_none()`
- `src/primitives/stream.rs:44, 72, 124` — All three primitives use this validation
- **Confirmed:** A tls-conn struct (not a Port ExternalObject) will be rejected with `type-error`

### ✅ Coroutine/Stream Protocol

**Claim:** `port/lines`, `stream/map`, `stream/filter`, `stream/collect` all work on coroutines via `coro/resume`/`coro/done?`/`coro/value`. Custom coroutines created via `coro/new` would work with all stream combinators.

**Evidence:**
- `stdlib.lisp:907-915` — `port/lines` creates coroutine via `coro/new`, yields via `stream/read-line`
- `stdlib.lisp:917-925` — `port/chunks` creates coroutine via `coro/new`, yields via `stream/read`
- `stdlib.lisp:822-830` — `stream/map` resumes source via `coro/resume`, checks `coro/done?`, gets value via `coro/value`
- `stdlib.lisp:832-840` — `stream/filter` uses same pattern
- **Confirmed:** The coroutine protocol is universal. Custom coroutines work with all stream combinators.

### ✅ Plugin Primitives Return Signature

**Claim:** Primitives return `(SignalBits, Value)`.

**Evidence:**
- `src/plugin.rs:71` — `PluginInitFn` type signature
- `src/primitives/def.rs` — `NativeFn = fn(&[Value]) -> (SignalBits, Value)`
- `plugins/crypto/src/lib.rs:68-84` — Hash primitives return `(SIG_OK, Value::bytes(...))`
- **Confirmed:** All primitives return `(SignalBits, Value)` tuple.

### ✅ ExternalObject Pattern

**Claim:** Plan's use of `Value::external("tls-state", ...)` matches how other plugins create ExternalObject values.

**Evidence:**
- `plugins/crypto/src/lib.rs:83` — `Value::bytes(hash.to_vec())` for simple values
- `src/plugin.rs:30` — `Value::struct_from(fields)` for struct returns
- Pattern: Plugins use `Value::external(type_name, rust_object)` for opaque types
- **Confirmed:** The pattern is correct. No examples in crypto plugin, but the API exists in `Value`.

### ✅ TCP/Connect Return Value

**Claim:** `tcp/connect` returns a Port ExternalObject, and `stream/read`/`stream/write` work on it.

**Evidence:**
- `src/primitives/net.rs:263-280` — `tcp/connect` returns `(SIG_YIELD | SIG_IO, IoRequest::new(IoOp::Connect {...}, Value::NIL))`
- The backend creates a Port and returns it as completion value
- `src/primitives/stream.rs:44, 72` — `stream/read-line` and `stream/read` validate port via `as_external::<Port>()`
- **Confirmed:** TCP ports work with stream primitives.

### ✅ Stream/Read and Stream/Write Yield SIG_IO

**Claim:** `stream/read`/`stream/write` require a scheduler context (they yield `SIG_IO`).

**Evidence:**
- `src/primitives/stream.rs:53-55` — `stream/read-line` returns `(SIG_YIELD | SIG_IO, IoRequest::...)`
- `src/primitives/stream.rs:104-107` — `stream/read` returns `(SIG_YIELD | SIG_IO, IoRequest::...)`
- `src/primitives/stream.rs:139-150` — `stream/write` returns `(SIG_YIELD | SIG_IO, IoRequest::...)`
- **Confirmed:** All stream I/O primitives yield `SIG_IO` and require scheduler context.

### ✅ Plugin Loading Pattern (Partial)

**Claim:** Plugins are loaded via `(import "path/to/plugin.so")`.

**Evidence:**
- `src/primitives/modules.rs:78-82` — `prim_import_file` detects `.so` extension and calls `load_plugin`
- `src/plugin.rs:81-149` — `load_plugin` function loads `.so` and calls `elle_plugin_init`
- **Confirmed:** Plugins are loaded via `(import "path/to/plugin.so")`, not `(load "lib/tls.lisp")`

---

## Critical Issues

### ❌ ISSUE 1: Missing `bytes/find` Primitive

**Claim:** Plan depends on `bytes/find` for line-oriented reads (section 4.4, line 599).

**Evidence:**
- `src/primitives/bytes.rs` — No `bytes/find` primitive exists
- Grep search: No `bytes/find` or `bytes-find` in codebase
- `stdlib.lisp:132-148` — `find` exists for sequences, but not for bytes specifically
- **Impact:** The plan's `tls/read-line` implementation (line 599) calls `(bytes/find buf 10)` which does not exist

**Workaround:** Either:
1. Implement `bytes/find` as a primitive (searches for a byte value in bytes)
2. Use `string/find` on the decoded plaintext (requires UTF-8 assumption)
3. Implement line-finding in Elle code using `get` and iteration

**Recommendation:** Add `bytes/find` primitive. It's a natural operation and useful beyond TLS.

---

### ❌ ISSUE 2: Mutable Bytes Operations Unclear

**Claim:** Plan uses `@bytes` (mutable bytes) with `push-bytes` and `pop-bytes` operations (section 4.4, lines 565, 585).

**Evidence:**
- `src/primitives/bytes.rs:14-59` — `prim_bytes` and `prim_blob` create bytes/mutable bytes
- No `push-bytes` or `pop-bytes` primitives exist
- `src/primitives/array.rs` — `push` and `pop` exist for arrays, not bytes
- **Impact:** The plan's read buffer implementation (lines 565, 585) uses undefined operations

**Workaround:** The plan acknowledges this (lines 645-680) and suggests using a list of byte chunks instead. This is the correct approach.

**Recommendation:** Clarify in the final design that the read buffer is a list of byte chunks, not a mutable bytes value. Or implement `bytes/push` and `bytes/pop` primitives.

---

### ❌ ISSUE 3: Plugin Loading Mechanism Incorrect

**Claim:** Plan says `(load "lib/tls.lisp")` loads the Elle stdlib (section 4.4, line 289).

**Evidence:**
- `src/primitives/modules.rs:8-157` — The `import` primitive (alias `import-file`) loads files
- No `load` primitive exists in Elle
- `stdlib.lisp` is loaded at startup by the VM, not via `(load ...)`
- **Impact:** Users cannot load `lib/tls.lisp` via `(load "lib/tls.lisp")` — they must use `(import "lib/tls.lisp")`

**Recommendation:** Change plan to use `(import "lib/tls.lisp")` instead of `(load "lib/tls.lisp")`.

---

### ⚠️ ISSUE 4: Plugin Initialization Timing

**Claim:** Plan assumes plugin primitives are available after `(import "plugin.so")`.

**Evidence:**
- `src/primitives/modules.rs:78-82` — `import` detects `.so` and calls `load_plugin`
- `src/plugin.rs:125-144` — Primitives are registered into `vm.docs` but NOT into the global symbol table
- **Impact:** Plugin primitives may not be immediately callable after import without explicit registration

**Recommendation:** Verify that `load_plugin` properly registers primitives into the VM's global namespace. The current code registers them into `vm.docs` (documentation), but it's unclear if they're registered as callable functions.

---

## Gaps and Assumptions

### 🔍 GAP 1: Mutable Bytes Buffer Strategy

**Plan says (lines 653-680):** "The exact buffer strategy depends on what `@bytes` primitives exist. If `@bytes` doesn't support efficient splice/drain, we'll use a list of byte chunks with offset tracking."

**Reality:** `@bytes` does NOT have `push-bytes` or `pop-bytes` primitives. The plan correctly identifies this as a gap but defers the decision.

**Recommendation:** Finalize the buffer strategy now:
- **Option A:** Use a list of byte chunks (simplest, matches DNS pattern)
- **Option B:** Implement `bytes/push` and `bytes/pop` primitives
- **Option C:** Use a mutable array of integers (each byte as an int)

The plan should commit to Option A (list of chunks) to match the DNS implementation pattern.

---

### 🔍 GAP 2: Error Handling in Handshake Loop

**Plan says (section 4.1, lines 312-313):** "When `stream/read` returns `nil`, raise `:tls-error`."

**Reality:** The plan doesn't specify what happens if `stream/read` itself errors (e.g., network timeout, I/O error). These errors should propagate naturally via Elle's signal system, but the plan should be explicit about this.

**Recommendation:** Add a note that I/O errors from `stream/read` and `stream/write` propagate as `:io-error` and are not caught by the handshake loop.

---

### 🔍 GAP 3: Outgoing Data During Reads (Section 4.6)

**Plan says (lines 693-717):** "TLS 1.3 post-handshake messages can produce outgoing data at any time. The read path must send this data."

**Reality:** This is correct and important. The plan correctly identifies this and includes it in `tls-fill-buffer`. However, the plan doesn't discuss what happens if `stream/write` fails while sending outgoing data. Should the read fail, or should the error be logged?

**Recommendation:** Clarify error handling: if `stream/write` fails while sending outgoing data, the error should propagate (not be silently swallowed).

---

### 🔍 GAP 4: Handshake Completion Detection

**Plan says (section 3.2, line 199):** "`tls/process` returns status `:ready` when handshake is complete."

**Reality:** The plan defines status keywords (lines 213-221) but doesn't specify the exact return struct format. The plan says (lines 207-211):
```
{:status keyword
 :plaintext bytes
 :outgoing-size int}
```

**Recommendation:** Verify that rustls's `UnbufferedStatus` enum maps cleanly to these status keywords. The plan should include the exact mapping.

---

### 🔍 GAP 5: Certificate Validation Options

**Plan says (section 3.1, lines 139-142):** Options include `:no-verify`, `:ca-file`, `:client-cert`, `:client-key`.

**Reality:** The plan defers these to "Certificate options" (section 9, line 1121). The plan should clarify:
- How `:ca-file` is loaded (PEM format? Multiple certs?)
- How `:client-cert` and `:client-key` are paired
- What happens if `:client-cert` is provided without `:client-key`

**Recommendation:** Finalize certificate handling before implementation.

---

## Naming Conflicts

### ✅ No Conflicts Found

Searched for existing functions that would conflict with the plan's primitives:
- `tls/client-state` — not found
- `tls/server-config` — not found
- `tls/server-state` — not found
- `tls/process` — not found
- `tls/encrypt` — not found
- `tls/get-outgoing` — not found
- `tls/get-plaintext` — not found
- `tls/handshake-complete?` — not found
- `tls/close-notify` — not found
- `tls/connect` — not found
- `tls/accept` — not found
- `tls/read` — not found
- `tls/write` — not found
- `tls/read-line` — not found
- `tls/read-all` — not found
- `tls/close` — not found
- `tls/lines` — not found
- `tls/chunks` — not found
- `tls/writer` — not found

**Confirmed:** No naming conflicts with existing Elle functions.

---

## Recommendations

### Priority 1: Must Fix Before Implementation

1. **Implement `bytes/find` primitive** or clarify the line-reading strategy
2. **Finalize buffer strategy** — commit to list-of-chunks approach
3. **Fix plugin loading documentation** — use `(import ...)` not `(load ...)`
4. **Verify plugin primitive registration** — ensure primitives are callable after import

### Priority 2: Should Clarify Before Implementation

1. **Error handling in handshake loop** — specify what happens on I/O errors
2. **Outgoing data error handling** — specify what happens if `stream/write` fails
3. **Certificate validation options** — finalize `:ca-file`, `:client-cert`, `:client-key` handling
4. **Status keyword mapping** — verify rustls `UnbufferedStatus` → Elle status keywords

### Priority 3: Nice to Have

1. **Add `bytes/find` primitive** — useful beyond TLS
2. **Add `bytes/push` and `bytes/pop` primitives** — useful for mutable bytes operations
3. **Document the sans-I/O pattern** — add to `docs/oddities.md` or `docs/cookbook.md`

---

## Conclusion

The plan is **architecturally correct** and follows Elle's design principles well. The sans-I/O approach is the right choice. However, **implementation cannot proceed** without:

1. Resolving the `bytes/find` gap
2. Finalizing the mutable bytes buffer strategy
3. Fixing the plugin loading documentation
4. Verifying plugin primitive registration

Once these are resolved, the implementation should be straightforward.
