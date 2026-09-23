# MCP `eval` tool

<!-- audited: 2026-09-23 -->

The `eval` tool collapses the MCP surface to a single verb: a monadic bind
over a persistent Elle image held in the server. Every call submits a
lambda and a list of input handles, the server applies the lambda to the
values behind those handles, and returns a new handle for the result plus
any captured output and metrics.

This document is the contract, as the server in the `mcp` submodule
implements it. Its tests are
[test-eval.lisp](https://github.com/elle-lisp/mcp/blob/main/test-eval.lisp) in
that repository.

## Why one verb

The other tools (`portrait`, `impact`, `trace`, `signal_query`, and the rest)
are useful, but 20 named tools is a menu agents have to memorize. `eval` lets
them be expressed as *well-known lambdas* the agent composes against the
image, the same way they would in the REPL.

The server reads the lambda's text, evaluates it to a callable, and applies
it to the input values. This document does the same with a portrait lambda:

```lisp
(def portrait-lambda
  "(fn []
     (let [portrait-lib ((import \"std/portrait\"))
           a (compile/analyze (file/read \"lib/http.lisp\") {:file \"lib/http.lisp\"})]
       (portrait-lib:render (portrait-lib:function a :http-request))))")

(def callable (eval (read portrait-lambda)))
(assert (string/contains? (callable) "http-request"))
```

Nothing about that expression needs a bespoke tool. The agent writes the
Elle, submits it through `eval`, and gets back a UUID naming the result.
Subsequent calls compose against that UUID.

A lambda is evaluated on its own. It reaches primitives, the standard library
and `import`, but not the server's own bindings, so the RDF store and the
server's analysis cache are out of its reach.

Large values (a compiled analysis, a huge string) never cross the JSON-RPC
wire. They live in the image behind a handle and the agent probes them by
submitting more lambdas.

## Request schema

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "tools/call",
  "params": {
    "name": "eval",
    "arguments": {
      "lambda":  "(fn [prev] (take 10 prev))",
      "inputs":  ["5b0e…", "9c41…"],
      "timeout_ms": 5000
    }
  }
}
```

**`lambda`** (string, required) — Elle source text. Must read as a single
expression that evaluates to a callable. Typical shapes:

- `(fn [prev] ...)` — unary, consumes one input handle
- `(fn [] (... compute from scratch ...))` — nullary
- `(fn [a b c] ...)` — n-ary; arity must match `(length inputs)`
- `(fn [& args] ...)` — rest-arg form; `args` collects the input values

**`inputs`** (array of strings, optional; default `[]`) — handles produced
by prior `eval` calls. Resolved positionally and passed as arguments to
the lambda. An unknown handle returns an error before the lambda runs.

**`timeout_ms`** (integer, optional; default `10000`) — wall-clock limit.
If the lambda has not returned by this deadline, it is cancelled, output
captured so far is returned, and the result is an error with `:error
:timeout`. Zero or negative disables the timeout.

## Response schema

`tools/call` always returns a JSON-RPC success envelope. The tool's own
success/failure lives inside `content[0].text` as a JSON document with
this shape:

```json
{
  "ok":           true,
  "handle":       "5b0e2f4a-…",
  "kind":         ":struct",
  "shape":        {"count": 10, "keys_sample": [":name", ":file"]},
  "stdout":       "hello\n",
  "stderr":       "",
  "duration_ns":  123456,
  "fibers":       []
}
```

| Field | Meaning |
|-------|---------|
| `ok` | `true` if the lambda returned normally, `false` if it raised or timed out (the handle still names the error value) |
| `handle` | UUID v4 addressing the result value in the image |
| `kind` | `":"` followed by the result's `type-of` — `:integer`, `:string`, `:array`, `:struct`, `:closure`, `:fiber` — or `:error` when `ok` is false. Enough to decide the next probe lambda |
| `shape` | Cheap structural hint: `count` for an array, list or set; `count` and up to five keys for a struct; `bytes` for a string; `reason` and `message` for an error. Never ships the value itself |
| `stdout` | Output written to `*stdout*` during the eval |
| `stderr` | Output written to `*stderr*` |
| `duration_ns` | Wall-clock nanoseconds the lambda ran (excluding handle bookkeeping) |
| `fibers` | Always empty today; see [Fibers](#fibers) |

When the server cannot run the lambda at all — a missing `lambda`, text that
does not read, a value that is not callable, an unknown handle, an arity
mismatch — the tool answers `isError: true` content whose text is `Internal
error:` followed by the message. No handle is created. The `ok: false` path is
reserved for *the lambda itself raised* — which is not a protocol error, just a
value the agent may want to inspect.

## Handle semantics

- Handles are UUID v4 strings. They address entries in a process-local
  handle table mapping `UUID → Value`.
- Handles are stable for the lifetime of the server process.
- Nothing is evicted. Every eval adds an entry. This is acceptable for an
  interactive agent session; a `pin`/`unpin` plus LRU scheme is not built.
- A handle names exactly one Elle value. That value may be shared
  structurally with other handles — the image does not copy on bind.
- An error handle names the value the lambda raised, usually a struct with
  `:error` and `:message`. A timeout names `{:error :timeout :message
  "operation timed out"}`.

## Output capture

Inside the lambda, `*stdout*` and `*stderr*` are rebound with `parameterize`
to ports on two scratch files under `.elle-mcp/`. The server reads the files
back after the lambda returns, ships their contents as the `stdout` and
`stderr` fields, and deletes them.

This means `println`, `eprintln` and `print` "just work" — the agent does not
need to redirect anything manually. The server's own JSON-RPC stdio is
unaffected; the response envelope still ships on the real stdout. `*stdin*`
is not rebound.

## Capabilities

The lambda inherits the server's full capabilities (FS, exec, network, FFI).
This matches the other tools: `verify_invariants` already evals the
invariants file with full privileges, and `test_run` shells out.

A sandboxed eval would accept a `deny` mask in the request, run the lambda in
a child fiber made with `fiber/new body mask :deny ...`, and surface
`:capability-denied` signals as structured errors. It is not built.

## Fibers

A lambda may spawn fibers with `fiber/new`, `ev/spawn`, and the like. The
server does not collect them: the `fibers` field is always empty, and a fiber
the lambda did not finish is reachable only through a value the lambda
returned.

## Errors

Three error tiers, each surfaced differently:

1. **Protocol error** — the server could not run the lambda (see
   [Response schema](#response-schema)). Returned as `isError: true`
   content. No handle is created.

2. **Lambda raised** — the lambda ran and raised. Returned as `ok: false`
   with a valid `handle` naming the error value. `kind` is `:error`.
   The agent inspects via further eval, e.g. `(fn [e] (get e :message))`.

3. **Timeout** — the lambda was still running when `timeout_ms` elapsed.
   The fiber is cancelled, captured output up to that point is returned,
   and the handle names `{:error :timeout ...}`.

Tiers 2 and 3 return the `stdout` and `stderr` captured up to the point of
failure. Partial output is diagnostically valuable.

## Example session

```text
# Analyze a file once, keep the analysis in the image
eval(lambda="(fn [] (compile/analyze (file/read \"lib/http.lisp\") {:file \"lib/http.lisp\"}))")
  -> {ok:true, handle:"5b0e…", kind:":analysis", shape:null}

# Probe it without shipping it: which functions does it define?
eval(lambda="(fn [a] (map (fn [s] (get s :name)) (compile/symbols a)))", inputs:["5b0e…"])
  -> {ok:true, handle:"9c41…", kind:":array", shape:{count: …}}

# Render a sample as text
eval(lambda="(fn [names] (string/join (take 3 names) \"\\n\"))", inputs:["9c41…"])
  -> {ok:true, handle:"e7d2…", kind:":string", shape:{bytes: …}}

# Print it, and read it from stdout
eval(lambda="(fn [s] (println s) :printed)", inputs:["e7d2…"])
  -> {ok:true, …, stdout:"…"}
```

## Non-goals

- **Cross-session handle persistence.** Handles live only for the server
  process. Reconnect loses the image. Persistence to disk (pinning +
  serializing values) is deferred.
- **Pure-lambda memoization.** Submitting the same lambda + inputs twice
  creates two handles. Deduping on `(lambda-hash, input-uuids)` when the
  lambda is signal-pure is a future optimization.
- **Multi-client isolation.** One image per server process. Two clients
  share the handle table. Per-client namespaces are deferred.
- **Shipping the value directly.** The response never serializes the
  result value — only `kind` and `shape`. If the agent wants the value,
  they `eval` a lambda that prints something JSON-shaped and read `stdout`.

## Supersedes

What the other tools compute from an analysis, a lambda can compute too: they
call `compile/analyze`, `compile/signal`, `std/portrait` and their kin, which a
lambda reaches. The graph tools are the exception, because the RDF store is a
server binding a lambda cannot see. The specialized tools remain, to avoid
breaking clients. The long-term direction is to phase them out in favor of eval
+ well-known lambdas.
