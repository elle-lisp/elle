#!/usr/bin/env elle
(elle/epoch 9)

## Test suite for lib/color — color science library

(def color ((import "std/color")))

# ── Construction ─────────────────────────────────────────────────────

(def red (color:rgb 1.0 0.0 0.0))
(assert (= (get red :space) :srgb) "rgb constructs srgb")
(assert (= (get red :r) 1.0) "rgb red component")

(def blue-a (color:rgba 0.0 0.0 1.0 0.5))
(assert (= (get blue-a :a) 0.5) "rgba alpha")

(def hsl-blue (color:hsl 240.0 1.0 0.5))
(assert (= (get hsl-blue :space) :hsl) "hsl constructs hsl")

# ── sRGB ↔ HSL roundtrip ────────────────────────────────────────────

(defn approx [a b tol]
  (< (abs (- a b)) tol))

(def red-hsl (color:convert red :hsl))
(assert (approx (get red-hsl :h) 0.0 1.0) "red hue ~0")
(assert (approx (get red-hsl :s) 1.0 0.01) "red saturation ~1")
(assert (approx (get red-hsl :l) 0.5 0.01) "red lightness ~0.5")

(def red-back (color:convert red-hsl :srgb))
(assert (approx (get red-back :r) 1.0 0.01) "HSL→sRGB roundtrip r")
(assert (approx (get red-back :g) 0.0 0.01) "HSL→sRGB roundtrip g")
(assert (approx (get red-back :b) 0.0 0.01) "HSL→sRGB roundtrip b")

# ── sRGB ↔ Lab roundtrip ────────────────────────────────────────────

(def red-lab (color:convert red :lab))
(assert (approx (get red-lab :l) 53.23 1.0) "red Lab L ~53")
(assert (> (get red-lab :a) 60.0) "red Lab a is positive (toward red)")

(def red-from-lab (color:convert red-lab :srgb))
(assert (approx (get red-from-lab :r) 1.0 0.02) "Lab→sRGB roundtrip r")
(assert (approx (get red-from-lab :g) 0.0 0.02) "Lab→sRGB roundtrip g")

# ── sRGB ↔ Oklch roundtrip ──────────────────────────────────────────

(def red-oklch (color:convert red :oklch))
(assert (> (get red-oklch :l) 0.0) "oklch L > 0")
(assert (> (get red-oklch :c) 0.0) "oklch chroma > 0")

(def red-from-oklch (color:convert red-oklch :srgb))
(assert (approx (get red-from-oklch :r) 1.0 0.02) "Oklch→sRGB roundtrip r")
(assert (approx (get red-from-oklch :g) 0.0 0.05) "Oklch→sRGB roundtrip g")

# ── Mixing ───────────────────────────────────────────────────────────

(def blue (color:rgb 0.0 0.0 1.0))
(def purple (color:mix red blue 0.5))
(assert (approx (get purple :r) 0.5 0.01) "mix r")
(assert (approx (get purple :b) 0.5 0.01) "mix b")

(def all-red (color:mix red blue 0.0))
(assert (approx (get all-red :r) 1.0 0.01) "mix t=0 is c1")

# ── Gradient ─────────────────────────────────────────────────────────

(def grad (color:gradient red blue 5))
(assert (= (length grad) 5) "gradient length")
(assert (approx (get (grad 0) :r) 1.0 0.01) "gradient start is c1")
(assert (approx (get (grad 4) :b) 1.0 0.01) "gradient end is c2")

# ── Lighten / Darken ─────────────────────────────────────────────────

(def dark-red (color:rgb 0.5 0.0 0.0))
(def lighter (color:lighten dark-red 0.2))
(assert (> (get (color:convert lighter :hsl) :l)
           (get (color:convert dark-red :hsl) :l))
        "lighten increases lightness")

(def darker (color:darken dark-red 0.1))
(assert (< (get (color:convert darker :hsl) :l)
           (get (color:convert dark-red :hsl) :l))
        "darken decreases lightness")

# ── Saturate / Desaturate ────────────────────────────────────────────

(def muted (color:rgb 0.5 0.3 0.3))
(def more-sat (color:saturate muted 0.2))
(assert (> (get (color:convert more-sat :hsl) :s)
           (get (color:convert muted :hsl) :s))
        "saturate increases saturation")

# ── Complement ───────────────────────────────────────────────────────

(def comp (color:complement red))
(def comp-hsl (color:convert comp :hsl))
(assert (approx (get comp-hsl :h) 180.0 2.0) "complement of red is cyan")

# ── Distance ─────────────────────────────────────────────────────────

(def d-same (color:distance red red))
(assert (approx d-same 0.0 0.01) "distance to self is 0")

(def green (color:rgb 0.0 1.0 0.0))
(def d-diff (color:distance red green))
(assert (> d-diff 50.0) "red-green distance is large")

# ── Pixel interop ───────────────────────────────────────────────────

(def px (color:to-rgba8 red))
(assert (= (px 0) 255) "to-rgba8 r=255")
(assert (= (px 1) 0) "to-rgba8 g=0")
(assert (= (px 3) 255) "to-rgba8 a=255 default")

(def from-px (color:from-rgba8 255 128 0 255))
(assert (approx (get from-px :r) 1.0 0.01) "from-rgba8 r")
(assert (approx (get from-px :g) 0.502 0.01) "from-rgba8 g")

(println "color: all tests passed")
