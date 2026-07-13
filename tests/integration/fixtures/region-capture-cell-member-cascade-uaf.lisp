(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-member-cascade-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because it SIGSEGVs under
# --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault would take the whole harness down. It is exercised by
# the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_capture_cell_member_cascade_uaf`).
#
# WHAT IT REPRODUCES — a CASCADE over-free of a struct MEMBER stored into a
# captured cell. A server fiber reads a request off a socket, stores a MEMBER of
# the parsed request (`(get req :params)`) into a module-level `@`-capture cell,
# then reads a SIBLING member (`(get req :id)`) to build the reply. That sibling
# read is `req`'s last use, so the solver frees `req`'s region — and because the
# stored `:params` member's region is a CHILD of `req`'s, it CASCADE-frees under
# the still-live cell. `(assign captured (get req :params))` should have pinned
# the member's region live independent of its parent; it does not, so:
#
#     [guardfree] SIGSEGV — use-after-free
#     freed by region N via cascade(M)      <- :params freed when its parent req went
#     free site:  unknown                   <- cascade frees carry no direct site
#     context:    UpdateCapture             <- the `@`-cell assign
#
# ISOLATING INGREDIENTS (dropping any ONE makes it guardfree-clean):
#   * the cell stores a MEMBER of `req` (`(get req :params)`), not the whole
#     value — storing `req` itself keeps the parent live, so no cascade;
#   * a SIBLING member of the SAME `req` is read AFTER the store (`(get req :id)`,
#     here into the JSON reply) — that read is what ends `req`'s liveness; a reply
#     that does not touch `req` again is clean;
#   * the value ARRIVES OVER A SOCKET into a spawned fiber and the reply is
#     WRITTEN BACK to it — the identical shapes with an in-process literal / a
#     json/parse of a literal string are clean, so this is specific to the
#     port-read value's region living under the connection fiber.
#
# ORIGIN — mu's lib/cont/ipc.lisp: each JSON-RPC driver callback (on-retire,
# on-owner-summarize, on-tool-action, …) does exactly this — `(assign got-X params)`
# of the RPC request's params member, while the same dispatch reads the request's
# id/method to frame the reply. --trace=guardfree on the mu suites detonates here
# for ipc, ipc-roundtrip, spawn-agent, and adopt-grant (all cascade / UpdateCapture);
# without guardfree the freed member's page is silently reused (a later `get` on the
# cell read a `closure-template` out of it) — a latent data-corruption bug the
# normal `make smoke` gate reads straight past.
#
# WHEN FIXED — storing a struct member into a captured cell pins that member's
# region live independent of its parent, so the sibling read no longer cascade-frees
# it — this exits 0 under guardfree; the pin in elle_scripts.rs then passes. Distinct
# from the direct-free capture-cell string over-free
# (`region_capture_cell_string_accum_uaf`); this is the cascade / stored-member path.

(def @captured nil)

(defn handle-connection [conn]
  # Mirrors lib/cont/ipc.lisp handle-connection + a driver callback: read a
  # request, store its :params MEMBER in the cell, then read a SIBLING member
  # (:id) to frame the reply written back over the connection.
  (let [line (port/read-line conn)
        req (json/parse line :keys :keyword)]
    (assign captured (get req :params {}))
    (protect (begin
               (port/write conn
                           (string (json/serialize {:jsonrpc "2.0"
                                   :id (get req :id)
                                   :result "ack"}) "\n"))
               (port/flush conn)))
    (protect (port/close conn))))

# Short basename: unix sun_path is capped at 108 bytes (see tests/elle/net.lisp).
(with-temp-dir d
               (let [sock (path/join d "u.sock")
                     listener (unix/listen sock)]
                 (ev/spawn (fn []
                             (protect (handle-connection (unix/accept listener)))))
                 (ev/sleep 0.03)
                 (let [s (unix/connect sock)]
                   (port/write s
                               (string (json/serialize {:jsonrpc "2.0"
                                       :id 1
                                       :method "retire"
                                       :params {:done true :who "child"}}) "\n"))
                   (port/flush s)
                   (port/read-line s)
                   (protect (port/close s)))
                 (ev/sleep 0.06)))

# Reached only when the member's region survives the sibling read — i.e. once
# fixed. Reading the cell here is itself a deref of the (currently freed) member.
(println (string "region-capture-cell-member-cascade-uaf: captured="
                 (get captured :done nil)))
