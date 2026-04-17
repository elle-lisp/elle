# MCP `eval` tool

The `eval` tool collapses the MCP surface to a single verb: a monadic bind
over a persistent Elle image held in the server. Every call submits a
lambda and a list of input handles, the server applies the lambda to the
values behind those handles, and returns a new handle for the result plus
any captured I/O and metrics.

This document is the contract. Tests in [`tools/test-mcp-eval.lisp`](../tools/test-mcp-eval.lisp) verify it.

## Why one verb

The existing tools (`portrait`, `impact`, `trace`, `signal_query`, etc.) are
useful, but 20 named tools is a menu agents have to memorize. `eval` lets
them be expressed as *well-known lambdas* the agent composes against the
image, the same way they would in the REPL:

```
(portrait (analyze "lib/http.lisp") :request)
```

Nothing about that expression needs a bespoke tool. The agent writes the
Elle, submits it through `eval`, and gets back a UUID naming the result.
Subsequent calls compose against that UUID.

Large values (a 1M-row SPARQL result, a compiled analysis, a huge string)
never cross the JSON-RPC wire. They live in the image behind a handle and
the agent probes them by submitting more lambdas.

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
      "inputs":  ["01JFH...", "01JFK..."],
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
- `(fn [& args] ...)` — rest-arg form; `args` is the list of input values

**`inputs`** (array of strings, optional; default `[]`) — handles produced
by prior `eval` calls. Resolved positionally and passed as arguments to
the lambda. An unknown handle returns a structured error before the
lambda runs.

**`timeout_ms`** (integer, optional; default `10000`) — wall-clock limit.
If the lambda has not returned by this deadline, it is cancelled, stdout
captured so far is returned, and the result is an error handle with
`:reason :timeout`. Zero or negative disables the timeout.

## Response schema

`tools/call` always returns a JSON-RPC success envelope. The tool's own
success/failure lives inside `content[0].text` as a JSON document with
this shape:

```json
{
  "ok":           true,
  "handle":       "01JFH8K2CQZ...",
  "kind":         ":map",
  "shape":        {"count": 10, "keys_sample": [":name", ":file"]},
  "stdout":       "hello\n",
  "stderr":       "",
  "duration_ns":  123456,
  "fibers":       []
}
```

| Field | Meaning |
|-------|---------|
| `ok` | `true` if the lambda returned normally, `false` if it threw (handle still valid — names the error value) |
| `handle` | ULID/UUID addressing the result value in the image |
| `kind` | Top-level type of the result as a keyword string — `:int`, `:string`, `:list`, `:map`, `:struct`, `:closure`, `:error`, `:nil`, `:fiber`, etc. Enough to decide the next probe lambda |
| `shape` | Cheap structural hint: for collections, `count` and a sample; for structs, the trait name; for errors, `:reason`. Never ships the value itself |
| `stdout` | Captured `*stdout*` output produced during the eval |
| `stderr` | Captured `*stderr*` output |
| `duration_ns` | Wall-clock nanoseconds the lambda ran (excluding handle bookkeeping) |
| `fibers` | Handles of fibers the lambda spawned but did not join. Each is itself a valid handle to an image value (`:kind :fiber`) |

On protocol-level errors (malformed JSON, unknown handle, lambda that
fails to parse) the tool returns `isError: true` content with a
structured message. The `ok: false` path is reserved for *the lambda
itself threw* — which is not a protocol error, just a value the agent
may want to inspect.

## Handle semantics

- Handles are ULIDs (sortable, URL-safe, 26 chars). They address entries
  in a process-local handle table mapping `ULID → Value`.
- Handles are stable for the lifetime of the server process. A handle
  from five minutes ago is still valid if nothing has evicted it.
- MVP retention: nothing is evicted. Every eval adds an entry. This is
  acceptable for an interactive agent session; a future `pin`/`unpin` +
  LRU will land when memory pressure becomes a concern.
- A handle names exactly one Elle value. That value may be shared
  structurally with other handles — the image does not copy on bind.
- `:kind :error` handles carry a struct with at least `:reason`
  (keyword), `:message` (string), `:caret` (string; source context when
  available). Further fields may include `:locals` (map), `:fiber-mask`
  (set), `:source-location` (string).

## I/O rebinding

Inside the lambda, `*stdout*`, `*stderr*`, and `*stdin*` are rebound via
`parameterize` to in-memory byte buffers:

- Writes to `*stdout*` / `*stderr*` accumulate in buffers; their contents
  ship back as the `stdout` / `stderr` fields.
- `*stdin*` is bound to an empty port; reads return `nil` (EOF)
  immediately. Interactive input is out of scope.

This means `println`, `eprintln`, `print`, `(io/write *stdout* ...)` all
"just work" — the agent does not need to redirect anything manually. The
server's own JSON-RPC stdio (saved at server startup) is unaffected; the
response envelope still ships on the real stdout.

## Capabilities

MVP: the lambda inherits the server's full capabilities (FS, exec,
network, FFI). This matches every existing tool — e.g. `verify_invariants`
already evals user-supplied code with full privileges, `test_run` shells
out.

Future (tracked as a risk, not required for MVP): accept a `deny` mask in
the request, spawn the lambda in a child fiber with `fiber/new body mask
:deny ...`, and surface `:capability-denied` signals as structured errors.
This is the general-purpose sandbox form of eval.

## Fibers

A lambda may spawn fibers with `fiber/new`, `ev/spawn`, etc. When the
top-level lambda returns:

- Fibers that have completed are forgotten.
- Fibers still runnable or paused are captured as handles (each handle
  names the fiber object, `:kind :fiber`). They continue to run in the
  server's scheduler.
- The agent can submit further lambdas against those fiber handles —
  e.g. `(fn [f] (fiber/status f))`, `(fn [f] (fiber/join f))`,
  `(fn [f] (fiber/cancel f))`.

The `fibers` field in the response is in creation order. It is the agent's
responsibility to track or cancel them; the server does not garbage-collect
live fibers.

## Errors

Three error tiers, each surfaced differently:

1. **Protocol error** — malformed JSON-RPC, unknown tool name,
   unparseable lambda, unknown input handle, arity mismatch, lambda did
   not evaluate to a callable. Returned as JSON-RPC `isError: true`
   content with `{:error <keyword> :message <string>}`. No handle is
   created.

2. **Lambda threw** — the lambda ran and raised. Returned as `ok: false`
   with a valid `handle` naming the error struct. `kind` is `:error`.
   The agent inspects via further eval, e.g.
   `(fn [e] (get e :caret))`.

3. **Timeout** — the lambda was still running when `timeout_ms` elapsed.
   The fiber is cancelled, captured output up to that point is returned,
   and a synthetic error handle is created with `:reason :timeout`.

All three tiers preserve captured `stdout` / `stderr` up to the point of
failure. Partial output is diagnostically valuable.

## Example session

```
# Submit nullary lambda, get a handle to a SPARQL result
eval(lambda="(fn [] (ox:query store \"SELECT ?name WHERE { ?p a <urn:elle:Fn> ; <urn:elle:name> ?name }\"))")
  -> {ok:true, handle:"01JFH...A", kind:":list", shape:{count: 3412}}

# Probe the shape without shipping the list
eval(lambda="(fn [rows] (take 3 rows))", inputs:["01JFH...A"])
  -> {ok:true, handle:"01JFH...B", kind:":list", shape:{count: 3},
      stdout:"",...}

# Render the sample as text, ship it
eval(lambda="(fn [sample] (string/join (map json/serialize sample) \"\\n\"))",
     inputs:["01JFH...B"])
  -> {ok:true, handle:"01JFH...C", kind:":string", shape:{bytes: 184}}

# Actually read the string (small enough to ship)
eval(lambda="(fn [s] s)", inputs:["01JFH...C"])
  -> {ok:true, handle:"01JFH...D", kind:":string", shape:{bytes: 184},
      stdout:"...",...}
```

## Non-goals (MVP)

- **Cross-session handle persistence.** Handles live only for the server
  process. Reconnect loses the image. Persistence to disk (pinning +
  serializing values) is deferred.
- **Pure-lambda memoization.** Submitting the same lambda + inputs twice
  creates two handles today. Deduping on `(lambda-hash, input-uuids)`
  when the lambda is signal-pure is a future optimization.
- **Multi-client isolation.** One image per server process. Two clients
  share the handle table. Per-client namespaces are deferred.
- **Shipping the value directly.** The response never serializes the
  result value — only `kind` and `shape`. If the agent wants the value,
  they `eval` a projection lambda that returns something JSON-shaped and
  read `stdout` or embed a `println`.

## Supersedes

Everything the existing 20 tools expose can be called from inside an eval
lambda (they're already Elle functions: `compile/analyze`,
`portrait-lib:render`, `ox:query`, etc.). The specialized tools remain
for now to avoid breaking clients — they are essentially cached wrappers.
The long-term direction is to phase them out in favor of eval +
well-known lambdas.
