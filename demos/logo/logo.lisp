#!/usr/bin/env elle

# Logo generator — the elle-lisp glyph rendered as colored fibers
#
# A leaning Y: three strokes meeting at a junction.
#   - Diagonal: top-right down to junction (dominant stroke)
#   - Arm: junction up to the left
#   - Base: junction horizontally to the right
#
# Each stroke is drawn as N parallel fibers offset from the centerline,
# in a yellow → red → dark gradient.
#
# Demonstrates: math, closures, structs, arrays, string building, file I/O


# ── Vector math ────────────────────────────────────────────────────

(defn v+ [a b]
  "Add two 2D vectors."
  [(+ (a 0) (b 0))
   (+ (a 1) (b 1))])

(defn v- [a b]
  "Subtract two 2D vectors."
  [(- (a 0) (b 0))
   (- (a 1) (b 1))])

(defn v* [s v]
  "Scale a 2D vector by scalar s."
  [(* s (v 0))
   (* s (v 1))])

(defn vlength [v]
  "Length of a 2D vector."
  (math/sqrt (+ (* (v 0) (v 0))
                (* (v 1) (v 1)))))


# ── Cubic bezier evaluation ────────────────────────────────────────

(defn bezier-eval [seg t]
  "Evaluate cubic bezier at parameter t ∈ [0,1].
   seg is {:p0 [x y] :p1 [x y] :p2 [x y] :p3 [x y]}."
  (let* ([u  (- 1.0 t)]
         [u2 (* u u)]
         [u3 (* u2 u)]
         [t2 (* t t)]
         [t3 (* t2 t)]
         [p0 seg:p0]
         [p1 seg:p1]
         [p2 seg:p2]
         [p3 seg:p3])
    (v+ (v+ (v* u3 p0)
            (v* (* 3.0 (* u2 t)) p1))
        (v+ (v* (* 3.0 (* u t2)) p2)
            (v* t3 p3)))))

(defn bezier-tangent [seg t]
  "Tangent vector of cubic bezier at parameter t."
  (let* ([u  (- 1.0 t)]
         [u2 (* u u)]
         [t2 (* t t)]
         [p0 seg:p0]
         [p1 seg:p1]
         [p2 seg:p2]
         [p3 seg:p3]
         # derivative: 3(1-t)^2(p1-p0) + 6(1-t)t(p2-p1) + 3t^2(p3-p2)
         [a (v* (* 3.0 u2) (v- p1 p0))]
         [b (v* (* 6.0 (* u t)) (v- p2 p1))]
         [c (v* (* 3.0 t2) (v- p3 p2))])
    (v+ a (v+ b c))))

(defn normal-at [seg t]
  "Unit normal (perpendicular to tangent, pointing left) at parameter t."
  (let* ([tang (bezier-tangent seg t)]
         [dx (tang 0)]
         [dy (tang 1)]
         [len (vlength tang)])
    (if (< len 0.001)
      [0.0 0.0]
      [(/ (- 0.0 dy) len)
       (/ dx len)])))

(defn offset-point [seg t dist]
  "Point offset from bezier centerline by dist along the normal."
  (let* ([pt (bezier-eval seg t)]
         [n  (normal-at seg t)])
    (v+ pt (v* dist n))))


# ── SVG path generation ────────────────────────────────────────────

(defn round2 [x]
  "Round a float to 1 decimal place for compact SVG output."
  (/ (math/round (* x 10.0)) 10.0))

(defn coord-str [pt]
  "Format a point as 'x,y' string."
  (-> (string (round2 (pt 0)))
      (append ",")
      (append (string (round2 (pt 1))))))

(defn taper [global-t]
  "Taper function: 0.3 at endpoints, 1 in the middle. Remapped sin(π·t)."
  (+ 0.3 (* 0.7 (math/sin (* (math/pi) global-t)))))

(defn fiber-path-data [segments dist nsteps]
  "Generate SVG path data string for a single fiber.
   segments is an array of bezier segment structs.
   dist is the max normal offset distance (tapered at endpoints).
   nsteps is samples per segment."
  (var path-str "")
  (var first-point true)
  (def nseg (length segments))
  (def total-steps (* nseg nsteps))
  (var seg-idx 0)
  (each seg in segments
    (var i 0)
    (while (<= i nsteps)
      (let* ([t (/ (float i) (float nsteps))]
             # global t across all segments for taper
             [global-t (/ (float (+ (* seg-idx nsteps) i)) (float total-steps))]
             [tapered-dist (* dist (taper global-t))]
             [pt (offset-point seg t tapered-dist)])
        (if first-point
          (begin
            (assign path-str (-> "M " (append (coord-str pt))))
            (assign first-point false))
          (assign path-str (-> path-str
                            (append " L ")
                            (append (coord-str pt))))))
      (assign i (+ i 1)))
    (assign seg-idx (+ seg-idx 1)))
  path-str)


# ── Centerline definition ──────────────────────────────────────────
#
# The glyph: a leaning Y.  WHY ELLE?
#
# Three strokes meeting at a junction point:
#   Stroke 1: arm — from junction up and to the left
#   Stroke 2: diagonal — from top-right down to the junction
#   Stroke 3: base — from junction horizontally to the right
#
# The junction sits at roughly (195, 360).
# No crossing, so no layering needed — all strokes are one layer.
#
# Each stroke has a gentle curve to keep it organic, not rigid.

(defn line->bezier [p0 p3]
  "Promote a line segment to a degenerate cubic bezier."
  (let* ([p1 (v+ p0 (v* (/ 1.0 3.0) (v- p3 p0)))]
         [p2 (v+ p0 (v* (/ 2.0 3.0) (v- p3 p0)))])
    {:p0 p0 :p1 p1 :p2 p2 :p3 p3}))

# Junction point — where the diagonal meets the base
# Raised 5% (26px) from previous position
(def junction [120.0 435.0])

# Diagonal: from top-right down to junction (the dominant stroke)
# Slight outward bow (control points pushed left of straight line)
(def diagonal @[
  {:p0 [390.0 54.0]
   :p1 [310.0 159.0]
   :p2 [210.0 319.0]
   :p3 [120.0 435.0]}
])

# Base: from junction horizontally to the right (the foot)
(def base @[
  (line->bezier [120.0 435.0] [420.0 432.0])
])

# Arm: branches off the diagonal at ~55% up (45% down), heading up-left
# Slight outward bow (control points pushed right of straight line)
(def arm @[
  {:p0 [268.0 225.0]
   :p1 [240.0 182.0]
   :p2 [182.0 114.0]
   :p3 [108.0 59.0]}
])


# ── Fiber configuration ────────────────────────────────────────────

(def num-fibers 7)
(def fiber-spread 66.0)    # total width of the fiber bundle
(def fiber-width 8.0)      # stroke width per fiber (gap ≈ spread/(n-1) - width)

# Fiber colors: yellow → red → black gradient
(def fiber-colors @[
  "#f5d020"    # bright yellow
  "#e8a215"    # gold
  "#d47020"    # orange
  "#c43c1e"    # red-orange
  "#a01818"    # red
  "#551010"    # dark red
  "#1a1a1a"    # black (matches background)
])

# All fibers at full opacity — color gradient provides the visual depth
(def fiber-opacities @[
  1.0  1.0  1.0  1.0  1.0  1.0  1.0
])

(def samples-per-segment 40)


# ── SVG assembly ───────────────────────────────────────────────────

(defn svg-path [path-data color opacity width]
  "Generate an SVG <path> element string."
  (-> "  <path d=\""
      (append path-data)
      (append "\" fill=\"none\" stroke=\"")
      (append color)
      (append "\" stroke-width=\"")
      (append (string width))
      (append "\" stroke-linecap=\"round\" opacity=\"")
      (append (string opacity))
      (append "\"/>")))

(defn render-fibers [segments]
  "Render all fibers for a set of bezier segments, returning array of SVG path strings."
  (var paths @[])
  (var i 0)
  (while (< i num-fibers)
    (let* ([frac (/ (float i) (float (- num-fibers 1)))]
           # offset ranges from -spread/2 to +spread/2
           [dist (- (* frac fiber-spread) (/ fiber-spread 2.0))]
           [color (fiber-colors i)]
           [opacity (fiber-opacities i)]
           [path-data (fiber-path-data segments dist samples-per-segment)]
           [svg (svg-path path-data color opacity fiber-width)])
      (push paths svg))
    (assign i (+ i 1)))
  paths)

(defn build-svg []
  "Assemble the complete SVG document."
  (var doc "")
  (assign doc (append doc "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 512 512\">\n"))

  # Three strokes — no layering needed, draw order doesn't matter
  (each path in (render-fibers diagonal)
    (assign doc (-> doc (append path) (append "\n"))))
  (each path in (render-fibers arm)
    (assign doc (-> doc (append path) (append "\n"))))
  (each path in (render-fibers base)
    (assign doc (-> doc (append path) (append "\n"))))

  (assign doc (append doc "</svg>\n"))
  doc)


# ── Output ─────────────────────────────────────────────────────────

(def output-path "docs/logo.svg")
(def svg (build-svg))
(spit output-path svg)
(print (-> "wrote " (append output-path)))
