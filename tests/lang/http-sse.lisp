(elle/epoch 13)
# audited: 2026-09-29
# Server-sent events over loopback: sse-get and sse-post read what http:sse-response streams.
# lib/http.md

(def http ((import "std/http")))

# ============================================================================
# Server-Sent Events: server emits, client fiber consumes
# ============================================================================

(def sse-listener (tcp/listen "127.0.0.1" 0))
(def sse-server-port
  (let [parts (string/split (port/path sse-listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn sse-test-handler [req]
  (case req:path
    "/stream"
      (http:sse-response (fn [send]
                           (send {:data "first"})
                           (send {:event "tick" :data "1" :id "a"})
                           (send {:event "tick" :data "2" :id "b"})
                           (send {:data "multi\nline"})))
    "/retry-then-done"
      (http:sse-response (fn [send]
                           (send {:retry 50 :id "x"})
                           (send {:event "once" :data "hello" :id "x"})))

    # /echo-post: POST-only handler that streams back a synthetic
    # chat-completion-style response, echoing the request body in the
    # first event so the test can verify end-to-end delivery.
    "/echo-post"
      (if (= req:method "POST")
        (http:sse-response (fn [send]
                             (send {:event "meta"
                                    :data (string "ct="
                                    (or (get req:headers :content-type) "none"))})
                             (send {:event "delta"
                                    :data (string "body=" (or req:body ""))})
                             (send {:event "delta" :data "token-1"})
                             (send {:event "delta" :data "token-2"})
                             (send {:event "done" :data "[DONE]"})))
        (http:respond 405 "method not allowed"))

    # /bad-post: POST handler that rejects with 400 so we can verify
    # sse-post's error surface.
    "/bad-post" (http:respond 400 "nope")

    (http:respond 404 "not found")))

(def sse-fiber (ev/spawn (fn [] (http:serve sse-listener sse-test-handler))))

# Basic stream: connect, consume all events, stop when stream ends.
(let [events @[]
      source (http:sse-get (string "http://127.0.0.1:" sse-server-port "/stream")
                           :reconnect false)]
  (each evt in source
    (push events evt))
  (assert (= (length events) 4) "sse: received all four events")
  (let [[e0 e1 e2 e3] events]
    (assert (= e0:event "message") "sse: first event defaults to 'message'")
    (assert (= e0:data "first") "sse: first event data")
    (assert (= e1:event "tick") "sse: named event type")
    (assert (= e1:id "a") "sse: id captured")
    (assert (= e2:id "b") "sse: id advances")
    (assert (= e3:data "multi\nline")
            "sse: multi-line data preserved as single payload")))

# ============================================================================
# sse-post: POST with body, consume streamed SSE response
# ============================================================================

# Happy path: POST a JSON body, expect events echoing the body.
(let [events @[]
      source (http:sse-post (string "http://127.0.0.1:" sse-server-port
                                    "/echo-post") "{\"prompt\":\"hi\"}")]
  (each evt in source
    (push events evt))
  (assert (= (length events) 5) "sse-post: received all five events")
  (let [[meta body t1 t2 done] events]
    (assert (= meta:event "meta") "sse-post: first event name")
    (assert (string/contains? meta:data "json")
            "sse-post: server saw Content-Type application/json by default")
    (assert (= body:event "delta") "sse-post: body echo event")
    (assert (= body:data "body={\"prompt\":\"hi\"}")
            "sse-post: request body delivered to server intact")
    (assert (= t1:data "token-1") "sse-post: token-1")
    (assert (= t2:data "token-2") "sse-post: token-2")
    (assert (= done:event "done") "sse-post: terminal event")
    (assert (= done:data "[DONE]")
            "sse-post: caller can detect OpenAI-style [DONE] sentinel")))

# Custom headers on sse-post are merged into the request.
(let [events @[]
      source (http:sse-post (string "http://127.0.0.1:" sse-server-port
                                    "/echo-post") "raw bytes"
                            :headers {:content-type "text/plain"})]
  (each evt in source
    (push events evt))
  (let [meta (first events)]
    (assert (string/contains? meta:data "text/plain")
            "sse-post: :headers override the default Content-Type")))

# Error path: non-2xx response signals :sse-bad-status (fiber body
# errors during draining; the error surfaces to the each-loop driver).
(let [[ok? err] (protect (let [source (http:sse-post (string "http://127.0.0.1:"
                               sse-server-port "/bad-post") "ignored")]
                           (each _ in source
                             nil)))]
  (assert (not ok?) "sse-post: non-2xx signals error")
  (assert (= err:reason :sse-bad-status) "sse-post: reason is :sse-bad-status")
  (assert (= err:status 400) "sse-post: reports server status"))

(port/close sse-listener)

(println "all http sse tests passed")
