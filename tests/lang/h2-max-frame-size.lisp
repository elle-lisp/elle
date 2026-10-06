(elle/epoch 14)
# audited: 2026-10-06
# :max-frame-size on http2:connect and http2:serve: refused out of range, advertised in range.
# lib/http2/overview.md
#
# A session advertises its max frame size in its first SETTINGS frame, and
# the peer stores what it read in the session's `:remote-settings`. Each
# case reads the advertisement back from the far side, so a value that
# reached the session but never reached the wire fails here.
#
# The refusals come before any connection. The client case connects to a
# port nothing listens on, and the server case hands `serve` a listener
# nobody connects to: a refusal checked after the connect would report
# the refused connection instead, and one checked after the accept would
# never return at all.

(def http2 ((import "std/http2")))

(def deadline 5)

(def default-max-frame (* 256 1024))

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)]
    [l (parse-int (slice p (+ 1 (string/find p ":"))))]))

(defn refused-max-frame? [result]
  "True when a `protect` result is the h2 refusal of a max frame size."
  (let [[ok? err] result]
    (and (not ok?) (= :h2-error (get err :error))
         (= :invalid-max-frame-size (get err :reason)))))

(defn with-session [serve-max connect-max body &named streaming]
  "Serve on a fresh port with `serve-max`, connect with `connect-max`, and
   hand `body` the client session."
  (let* [[listener lport] (listen-ephemeral)
         serve (if streaming http2:serve-streaming http2:serve)
         handler (if streaming
                   (fn [req ctrl]
                     (ctrl:send-headers 200)
                     (ctrl:end-stream))
                   (fn [req] {:status 200 :body "ok"}))
         sf (ev/spawn (fn []
                        (protect (serve listener handler
                                        :max-frame-size serve-max))))
         sess (http2:connect (concat "http://127.0.0.1:" (string lport))
                             :max-frame-size connect-max)]
    (defer
      (begin
        (protect (http2:close sess))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (body sess))))

# ── Out of range: refused before any connection ──────────────────────

(println "an out-of-range max frame size is refused before any connection...")

(let* [[l lport] (listen-ephemeral)
       url (concat "http://127.0.0.1:" (string lport))]
  (port/close l)
  (each bad in [16383 16777216 0]
    (assert (refused-max-frame? (protect (http2:connect url :max-frame-size bad)))
            (string "connect must refuse a max frame size of " bad))))

(let [[listener _] (listen-ephemeral)]
  (defer
    (protect (port/close listener))
    (each bad in [16383 16777216]
      (each serve in [http2:serve http2:serve-streaming]
        (let [r (ev/timeout deadline
                            (fn []
                              (protect (serve listener (fn [& _] nil)
                                       :max-frame-size bad))))]
          (assert (not (nil? r))
                  (string "serve must refuse " bad " rather than accept"))
          (assert (refused-max-frame? r)
                  (string "serve must refuse a max frame size of " bad)))))))

# ── In range: each side advertises its own ───────────────────────────

(println "each side advertises the max frame size it was given...")

(with-session 20000 16384
              (fn [sess]
                (assert (= 16384 (get sess:local-settings :max-frame-size))
                        "the client keeps the size it was given")
                (assert (= 20000 (get sess:remote-settings :max-frame-size))
                        "the client reads the size the server advertised")
                (assert (= 200 (get (http2:send sess "GET" "/") :status))
                        "a request still completes")))

(with-session 30000 nil
              (fn [sess]
                (assert (= 30000 (get sess:remote-settings :max-frame-size))
                        "serve-streaming advertises the size it was given"))
              :streaming true)

(with-session nil nil
              (fn [sess]
                (assert (= default-max-frame
                           (get sess:local-settings :max-frame-size))
                        "the client defaults to 256 KiB")
                (assert (= default-max-frame
                           (get sess:remote-settings :max-frame-size))
                        "the server defaults to 256 KiB")))

(println "h2-max-frame-size: refused out of range, advertised in range")
