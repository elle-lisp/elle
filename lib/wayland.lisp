(elle/epoch 8)
## lib/wayland.lisp — Wayland compositor interaction for Elle
##
## Wraps the elle-wayland plugin primitives with idiomatic Elle APIs.
## All I/O is async via ev/poll-fd and the fiber scheduler.
##
## Dependencies:
##   - elle-wayland plugin loaded via (import "plugin/wayland")
##
## Usage:
##   (def wl-plugin (import "plugin/wayland"))
##   (def wl ((import "std/wayland") wl-plugin))
##   (let [[conn (wl:connect)]]
##     (println "outputs:" (wl:outputs conn))
##     (wl:disconnect conn))

## ── Entry-point thunk ─────────────────────────────────────────────────

(fn [plugin]

  ## ── Connection ──────────────────────────────────────────────────────

  (defn wayland/connect []
    "Connect to the Wayland display server. Returns an opaque connection."
    (plugin:connect))

  (defn wayland/disconnect [conn]
    "Disconnect from the Wayland display server."
    (plugin:disconnect conn))

  (defn wayland/fd [conn]
    "Get the display file descriptor for ev/poll-fd."
    (plugin:display-fd conn))

  ## ── Event loop integration ──────────────────────────────────────────

  (defn wayland/dispatch [conn]
    "Dispatch pending Wayland events."
    (plugin:dispatch conn))

  (defn wayland/flush [conn]
    "Flush the Wayland connection."
    (plugin:flush conn))

  (defn wayland/poll-events [conn]
    "Drain buffered events as an array of structs."
    (plugin:poll-events conn))

  (defn wayland/event-loop [conn handler]
    "Run an event loop: poll fd, dispatch, drain events, call handler.
     Handler receives each event struct. Loop until handler returns :stop."
    (let [[fd (wayland/fd conn)]]
      (forever
        (ev/poll-fd fd 1)  ## POLLIN
        (wayland/dispatch conn)
        (wayland/flush conn)
        (for-each [ev (wayland/poll-events conn)]
          (when (= (handler ev) :stop)
            (break nil))))))

  ## ── Queries ─────────────────────────────────────────────────────────

  (defn wayland/outputs [conn]
    "List connected outputs."
    (plugin:outputs conn))

  (defn wayland/seats [conn]
    "List available seats."
    (plugin:seats conn))

  ## ── Layer shell ─────────────────────────────────────────────────────

  (defn wayland/layer-surface [conn &named width height anchor layer namespace]
    "Create a layer-shell surface."
    (plugin:layer-surface conn))

  (defn wayland/layer-configure [conn surface-id]
    "Acknowledge a layer surface configure."
    (plugin:layer-configure conn surface-id))

  (defn wayland/layer-destroy [conn surface-id]
    "Destroy a layer surface."
    (plugin:layer-destroy conn surface-id))

  ## ── Surface ops ─────────────────────────────────────────────────────

  (defn wayland/attach [conn surface-id buffer-id]
    "Attach a buffer to a surface."
    (plugin:attach conn surface-id buffer-id))

  (defn wayland/damage [conn surface-id x y width height]
    "Damage a region of a surface."
    (plugin:damage conn surface-id x y width height))

  (defn wayland/commit [conn surface-id]
    "Commit a surface."
    (plugin:commit conn surface-id))

  ## ── SHM buffers ─────────────────────────────────────────────────────

  (defn wayland/shm-buffer [conn width height]
    "Create an SHM buffer."
    (plugin:shm-buffer conn width height))

  (defn wayland/buffer-write [conn buffer-id offset data]
    "Write bytes to an SHM buffer at offset."
    (plugin:buffer-write conn buffer-id offset data))

  (defn wayland/buffer-fill [conn buffer-id color]
    "Fill an SHM buffer with an ARGB color."
    (plugin:buffer-fill conn buffer-id color))

  (defn wayland/buffer-destroy [conn buffer-id]
    "Destroy an SHM buffer."
    (plugin:buffer-destroy conn buffer-id))

  ## ── Screencopy ──────────────────────────────────────────────────────

  (defn wayland/screencopy [conn output-id]
    "Capture a screencopy frame from an output."
    (plugin:screencopy conn output-id))

  (defn wayland/screencopy-destroy [conn frame-id]
    "Destroy a screencopy frame."
    (plugin:screencopy-destroy conn frame-id))

  ## ── Foreign toplevel ────────────────────────────────────────────────

  (defn wayland/toplevels [conn]
    "List foreign toplevels (windows)."
    (plugin:toplevels conn))

  (defn wayland/toplevel-activate [conn toplevel-id seat-id]
    "Activate (focus) a toplevel window."
    (plugin:toplevel-activate conn toplevel-id seat-id))

  (defn wayland/toplevel-close [conn toplevel-id]
    "Request a toplevel window to close."
    (plugin:toplevel-close conn toplevel-id))

  (defn wayland/toplevel-subscribe [conn]
    "Subscribe to toplevel events."
    (plugin:toplevel-subscribe conn))

  ## ── Module struct ───────────────────────────────────────────────────

  {:connect wayland/connect
   :disconnect wayland/disconnect
   :fd wayland/fd
   :dispatch wayland/dispatch
   :flush wayland/flush
   :poll-events wayland/poll-events
   :event-loop wayland/event-loop
   :outputs wayland/outputs
   :seats wayland/seats
   :layer-surface wayland/layer-surface
   :layer-configure wayland/layer-configure
   :layer-destroy wayland/layer-destroy
   :attach wayland/attach
   :damage wayland/damage
   :commit wayland/commit
   :shm-buffer wayland/shm-buffer
   :buffer-write wayland/buffer-write
   :buffer-fill wayland/buffer-fill
   :buffer-destroy wayland/buffer-destroy
   :screencopy wayland/screencopy
   :screencopy-destroy wayland/screencopy-destroy
   :toplevels wayland/toplevels
   :toplevel-activate wayland/toplevel-activate
   :toplevel-close wayland/toplevel-close
   :toplevel-subscribe wayland/toplevel-subscribe})
