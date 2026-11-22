(elle/epoch 18)
## audited: 2026-11-22
## `elle lsp` speaks JSON-RPC on its standard streams: the lifecycle, diagnostics, hover, completion, and its version.
## tests/cli/overview.md
## src/lsp/AGENTS.md
##
## Each message is a `Content-Length` header, a blank line, then that many
## bytes of JSON. The server's stderr is discarded, so a chatty server cannot
## fill a pipe that nobody reads.

(include-file "../runner/lib/harness.lisp")

(defn start []
  (subprocess/exec elle ["lsp"] {:stderr :null}))

(defn send [server msg]
  "Write MSG, a struct, to SERVER as one framed message."
  (let [body (json/serialize msg)
        in (get server :stdin)]
    (port/write in
                (string "Content-Length: " (length (bytes body)) "\r\n\r\n" body))
    (port/flush in)))

(defn receive [server]
  "Read one framed message from SERVER, and answer it parsed."
  (let [out (get server :stdout)]
    (def @size nil)
    (forever
      (let [line (port/read-line out)]
        (assert (not (nil? line)) "the server closed its output mid-header")
        (let [text (string/trim (string line))]
          (when (empty? text) (break))
          (when (string/starts-with? text "Content-Length:")
            (assign
              size
              (parse-int (string/trim (slice text (length "Content-Length:")))))))))
    (assert (and size (> size 0)) "a message carries a Content-Length")
    (json/parse (string (port/read-exact out size)))))

(defn initialize [server]
  "Send initialize, and answer the result: the capabilities and serverInfo."
  (send server
        {:jsonrpc "2.0" :id 1 :method "initialize" :params {:capabilities {}}})
  (let [r (receive server)]
    (assert (= (get r "id") 1) (string "initialize answers its id: " r))
    (get r "result")))

(defn shutdown [server]
  "Send shutdown and exit, and require a clean exit."
  (send server {:jsonrpc "2.0" :id 999 :method "shutdown"})
  (let [r (receive server)]
    (assert (= (get r "id") 999) (string "shutdown answers its id: " r)))
  (send server {:jsonrpc "2.0" :method "exit"})
  (port/close (get server :stdin))
  (assert (= (subprocess/wait server) 0)
          "the server exits cleanly after shutdown"))

(defn open [server uri text]
  "Open URI holding TEXT, and answer the diagnostics the server publishes."
  (send server
        {:jsonrpc "2.0"
         :method "textDocument/didOpen"
         :params {:textDocument {:uri uri
                                 :languageId "elle"
                                 :version 1
                                 :text text}}})
  (let [d (receive server)]
    (assert (= (get d "method") "textDocument/publishDiagnostics")
            (string "an open publishes diagnostics: " d))
    d))

(defn first-start [diags]
  "The start line and character of the first diagnostic in DIAGS."
  (let [start (get (get (first (get (get diags "params") "diagnostics")) "range")
                   "start")]
    [(get start "line") (get start "character")]))

(eprintln "scenario: the server initializes and shuts down")
(let [s (start)
      caps (get (initialize s) "capabilities")]
  (assert (boolean? (get caps "hoverProvider")) "the server offers hover")
  (assert (= (get (get (get caps "completionProvider") "triggerCharacters") 0)
             "(") "completion triggers on an open paren")
  (shutdown s))

# The version has one source, `[package] version` in Cargo.toml.
(eprintln "scenario: the server names its version from the package version")
(let [s (start)
      info (get (initialize s) "serverInfo")
      version (get (string/split (first (filter
                                          (fn [l]
                                            (string/starts-with? l "version = "))
                                          (string/split (slurp "Cargo.toml")
                                            "\n"))) "\"") 1)]
  (assert (= (get info "name") "Elle Language Server") (string "got " info))
  (assert (= (get info "version") version)
          (string "serverInfo.version derives from Cargo.toml, got " info))
  (shutdown s))

(eprintln "scenario: a clean document publishes no diagnostics")
(let [s (start)]
  (initialize s)
  (let [d (open s "file:///test.lisp" "(+ 1 2)")]
    (assert (= (get (get d "params") "uri") "file:///test.lisp") "the uri")
    (assert (empty? (get (get d "params") "diagnostics"))
            (string "a clean document has no diagnostics: " d)))
  (shutdown s))

# The code is the one the lint CLI and `compile/diagnostics` read, so an editor
# and `elle lint` agree about a file. The position is 0-based, and an underflow
# would read 4294967295.
(eprintln "scenario: a syntax error's diagnostic sits on its 0-based line")
(let [s (start)]
  (initialize s)
  (let [d (open s "file:///bad.lisp" "((((")
        diags (get (get d "params") "diagnostics")
        [line character] (first-start d)]
    (assert (not (empty? diags)) "a syntax error has a diagnostic")
    (assert (= (get (first diags) "code") "E005") (string "got " diags))
    (assert (and (= line 0) (< character 1000))
            (string "the error is on the first line, 0-based: " diags)))
  (shutdown s))

(eprintln "scenario: hover answers for a known symbol")
(let [s (start)]
  (initialize s)
  (open s "file:///hover.lisp" "(def foo 42)")
  (send s
        {:jsonrpc "2.0"
         :id 2
         :method "textDocument/hover"
         :params {:textDocument {:uri "file:///hover.lisp"}
                  :position {:line 0 :character 5}}})
  (let [r (receive s)]
    (assert (= (get r "id") 2) "hover answers its id")
    (assert (struct? (get r "result")) (string "hover answers a result: " r)))
  (shutdown s))

(eprintln "scenario: completion offers the builtins")
(let [s (start)]
  (initialize s)
  (open s "file:///comp.lisp" "(+ 1 2)")
  (send s
        {:jsonrpc "2.0"
         :id 2
         :method "textDocument/completion"
         :params {:textDocument {:uri "file:///comp.lisp"}
                  :position {:line 0 :character 1}}})
  (let [r (receive s)
        items (get (get r "result") "items")]
    (assert (= (get r "id") 2) "completion answers its id")
    (assert (any? (fn [i] (= (get i "label") "+")) items)
            "completion includes +"))
  (shutdown s))

(eprintln "scenario: a change publishes new diagnostics")
(let [s (start)]
  (initialize s)
  (open s "file:///change.lisp" "(+ 1 2)")
  (send s
        {:jsonrpc "2.0"
         :method "textDocument/didChange"
         :params {:textDocument {:uri "file:///change.lisp" :version 2}
                  :contentChanges [{:text "((("}]}})
  (let [d (receive s)]
    (assert (= (get d "method") "textDocument/publishDiagnostics") "a change")
    (assert (not (empty? (get (get d "params") "diagnostics")))
            "broken code has diagnostics")
    (let [[line _] (first-start d)]
      (assert (< line 1000) (string "the line is reasonable: " line))))
  (shutdown s))

(eprintln "all lsp scenarios passed")
