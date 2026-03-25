#!/usr/bin/env elle

# Port I/O edge cases: zero-length reads/writes, negative counts, etc.

# ── Setup: TCP server ───────────────────────────────────────────────

(def listener (tcp/listen "127.0.0.1" 0))
(def port-num (integer (get (string/split (port/path listener) ":") 1)))

(def server-fiber (ev/spawn (fn []
  (forever
    (let [[[ok? conn] (protect (tcp/accept listener))]]
      (unless ok? (break nil))
      (ev/spawn (fn []
        (defer (protect (port/close conn))
          (port/write conn "hello world")
          (port/flush conn)
          (ev/sleep 5)))))))))

(defn fresh-conn []
  (tcp/connect "127.0.0.1" port-num))

# ── port/read edge cases ────────────────────────────────────────────

# 1. port/read 0 on TCP → empty bytes, no hang
(println "  1. port/read 0 on TCP...")
(let [[conn (fresh-conn)]]
  (let [[result (port/read conn 0)]]
    (assert (= (length result) 0) "port/read 0 returns empty")
    (assert (bytes? result) "port/read 0 returns bytes"))
  # Connection still usable
  (let [[data (port/read conn 5)]]
    (assert (= (string data) "hello") "port still usable after 0-read"))
  (port/close conn))
(println "  1. ok")

# 2. port/read 0 on file → empty bytes
(println "  2. port/read 0 on file...")
(spit "/tmp/elle-port-edge-test" "abc")
(let [[p (port/open "/tmp/elle-port-edge-test" :read)]]
  (let [[result (port/read p 0)]]
    (assert (= (length result) 0) "file port/read 0 returns empty")
    (assert (bytes? result) "file port/read 0 returns bytes"))
  (let [[data (port/read p 3)]]
    (assert (= (string data) "abc") "file port still usable"))
  (port/close p))
(println "  2. ok")

# 3. port/read negative → error
(println "  3. port/read negative...")
(let [[conn (fresh-conn)]]
  (let [[[ok? err] (protect (port/read conn (- 0 1)))]]
    (assert (not ok?) "port/read -1 is error")
    (assert (= err:error :value-error) "port/read -1 error kind"))
  (port/close conn))
(println "  3. ok")

# ── port/write edge cases ───────────────────────────────────────────

# 4. port/write empty string → no-op, returns 0
(println "  4. port/write empty string...")
(let [[conn (fresh-conn)]]
  (let [[n (port/write conn "")]]
    (assert (= n 0) "port/write '' returns 0"))
  (port/flush conn)
  (let [[data (port/read conn 5)]]
    (assert (= (string data) "hello") "port/write '' is no-op"))
  (port/close conn))
(println "  4. ok")

# 5. port/write empty bytes → no-op, returns 0
(println "  5. port/write empty bytes...")
(let [[conn (fresh-conn)]]
  (let [[n (port/write conn (bytes))]]
    (assert (= n 0) "port/write empty bytes returns 0"))
  (port/flush conn)
  (let [[data (port/read conn 5)]]
    (assert (= (string data) "hello") "port/write empty bytes is no-op"))
  (port/close conn))
(println "  5. ok")

# ── port/read-line edge cases ───────────────────────────────────────

# 6. port/read-line on empty file → nil (EOF)
(println "  6. port/read-line on empty file...")
(spit "/tmp/elle-port-edge-empty" "")
(let [[p (port/open "/tmp/elle-port-edge-empty" :read)]]
  (let [[result (port/read-line p)]]
    (assert (nil? result) "read-line on empty file is nil"))
  (port/close p))
(println "  6. ok")

# ── HTTP empty body response ────────────────────────────────────────

# 7. HTTP response with Content-Length: 0 → no hang
(def http ((import-file "./lib/http.lisp")))
(def received @[])
(defn handler [request]
  (push received request:body)
  (http:respond 200 ""))

(def http-listener (tcp/listen "127.0.0.1" 0))
(def http-port (integer (get (string/split (port/path http-listener) ":") 1)))
(def http-url (string "http://127.0.0.1:" http-port "/test"))
(def http-server (ev/spawn (fn [] (http:serve http-listener handler))))

(println "  7. HTTP empty body response...")
(let [[r (http:post http-url "payload")]]
  (assert (= r:status 200) "HTTP 200 with empty body")
  (assert (= r:body "") "body is empty string"))
(println "  7. ok")

(ev/abort http-server)
(port/close http-listener)

# ── Teardown ────────────────────────────────────────────────────────

(ev/abort server-fiber)
(port/close listener)

(println "")
(println "all port-edge-cases tests passed.")
