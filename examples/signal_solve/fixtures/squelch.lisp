(elle/epoch 16)
# audited: 2026-11-16
# A binding of squelch or attune: the solver filters the bits of the closure it wraps.
# examples/signal_solve/main.rs

# h is a binding an assign reaches, so wild may raise every bit. attune
# leaves it :yield, the :error the boundary may raise, and the :halt that
# crosses every boundary. Counter-factual: the one value that stands for
# every bit passed the mask whole, so use-tame raised every bit.
# expect use-tame |:yield :error :halt|
(def @h nil)
(assign h nil)
(defn wild []
  (h))
(def tame (attune :yield wild))
(defn use-tame []
  (tame))

# A squelch of :halt leaves the halt, which crosses every boundary.
# Counter-factual: the mask held :halt, so use-calm raised no halt.
# expect use-calm |:error :halt :ambient|
(defn quit []
  (sys/exit 0))
(def calm (squelch :halt quit))
(defn use-calm []
  (calm))
(fn [] {:use-tame use-tame :use-calm use-calm})
