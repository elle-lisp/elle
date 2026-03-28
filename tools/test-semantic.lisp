#!/usr/bin/env elle
## test-semantic.lisp — exercise the semantic MCP tools via subprocess
##
## Spawns the MCP server, sends requests, prints results.

(defn make-request [id method params]
  (json/serialize {:jsonrpc "2.0" :id id :method method :params params}))

(defn parse-response [line]
  (json/parse line))

(defn get-text [response]
  "Extract text content from an MCP tool result."
  (var result (get response "result"))
  (when result
    (var content (get result "content"))
    (when content
      (get (get content 0) "text"))))

# ── Spawn the server ────────────────────────────────────────────────────

(var proc (subprocess/exec "tools/run-elle.sh"
  ["tools/mcp-server.lisp"]
  {:stdin :pipe :stdout :pipe :stderr :null}))

(var pin  (get proc :stdin))
(var pout (get proc :stdout))

(defn send [id method params]
  (port/write pin (string (make-request id method params) "\n"))
  (port/flush pin))

(defn recv []
  (var line (port/read-line pout))
  (when (not (nil? line))
    (parse-response line)))

# ── Initialize ──────────────────────────────────────────────────────────

(send 1 "initialize" {})
(var init (recv))
(println "Server:" (get (get (get init "result") "serverInfo") "name")
         (get (get (get init "result") "serverInfo") "version"))
(println)

# ── Analyze a file ──────────────────────────────────────────────────────

(send 2 "tools/call" {"name" "analyze_file"
                       "arguments" {"path" "examples/signals.lisp"}})
(var r2 (recv))
(println "── analyze_file ──")
(println (get-text r2))
(println)

# ── Module portrait ─────────────────────────────────────────────────────

(send 3 "tools/call" {"name" "portrait"
                       "arguments" {"path" "examples/signals.lisp"}})
(var r3 (recv))
(println "── module portrait ──")
(println (get-text r3))
(println)

# ── Function portrait ───────────────────────────────────────────────────

(send 4 "tools/call" {"name" "portrait"
                       "arguments" {"path" "examples/signals.lisp"
                                    "function" "safe-map"}})
(var r4 (recv))
(println "── portrait: safe-map ──")
(println (get-text r4))
(println)

# ── Signal query ────────────────────────────────────────────────────────

(send 5 "tools/call" {"name" "signal_query"
                       "arguments" {"path" "examples/signals.lisp"
                                    "query" "silent"}})
(var r5 (recv))
(println "── signal_query: silent ──")
(println (get-text r5))
(println)

# ── Impact ──────────────────────────────────────────────────────────────

(send 6 "tools/call" {"name" "impact"
                       "arguments" {"path" "examples/functions.lisp"
                                    "function" "letter-grade"}})
(var r6 (recv))
(println "── impact: letter-grade ──")
(println (get-text r6))

# ── Cleanup ─────────────────────────────────────────────────────────────

(subprocess/kill proc)
(println)
(println "done")
