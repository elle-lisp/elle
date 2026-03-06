# Logo/Turtle Graphics Demo

## What This Demo Does

This demo generates an SVG image of the Elle logo using Bézier curves and vector math. It demonstrates:
- Vector arithmetic (addition, subtraction, scaling, length)
- Cubic Bézier curve evaluation and tangent computation
- Normal vector calculation for stroke offsetting
- SVG path generation
- File I/O with `spit`

The output is an SVG file (`docs/logo.svg`) showing a stylized "Y" shape (the Elle glyph) rendered with a fiber bundle effect — multiple parallel strokes with a yellow-to-black color gradient.

## How It Works

### Vector Math

The demo defines basic 2D vector operations:

```janet
(defn v+ [a b]
  "Add two 2D vectors."
  [(+ (get a 0) (get b 0))
   (+ (get a 1) (get b 1))])

(defn v- [a b]
  "Subtract two 2D vectors."
  [(- (get a 0) (get b 0))
   (- (get a 1) (get b 1))])

(defn v* [s v]
  "Scale a 2D vector by scalar s."
  [(* s (get v 0))
   (* s (get v 1))])

(defn vlength [v]
  "Length of a 2D vector."
  (math/sqrt (+ (* (get v 0) (get v 0))
                (* (get v 1) (get v 1)))))
```

### Cubic Bézier Curves

A cubic Bézier curve is defined by four control points (p0, p1, p2, p3) and a parameter t ∈ [0,1]:

```
B(t) = (1-t)³·p0 + 3(1-t)²t·p1 + 3(1-t)t²·p2 + t³·p3
```

```janet
(defn bezier-eval [seg t]
  "Evaluate cubic bezier at parameter t ∈ [0,1]."
  (let* ([u  (- 1.0 t)]
         [u2 (* u u)]
         [u3 (* u2 u)]
         [t2 (* t t)]
         [t3 (* t2 t)]
         [p0 (get seg :p0)]
         [p1 (get seg :p1)]
         [p2 (get seg :p2)]
         [p3 (get seg :p3)])
    (v+ (v+ (v* u3 p0)
            (v* (* 3.0 (* u2 t)) p1))
        (v+ (v* (* 3.0 (* u t2)) p2)
            (v* t3 p3)))))
```

The tangent vector (derivative) is:

```
B'(t) = 3(1-t)²(p1-p0) + 6(1-t)t(p2-p1) + 3t²(p3-p2)
```

```janet
(defn bezier-tangent [seg t]
  "Tangent vector of cubic bezier at parameter t."
  (let* ([u  (- 1.0 t)]
         [u2 (* u u)]
         [t2 (* t t)]
         [p0 (get seg :p0)]
         [p1 (get seg :p1)]
         [p2 (get seg :p2)]
         [p3 (get seg :p3)]
         [a (v* (* 3.0 u2) (v- p1 p0))]
         [b (v* (* 6.0 (* u t)) (v- p2 p1))]
         [c (v* (* 3.0 t2) (v- p3 p2))])
    (v+ a (v+ b c))))
```

### Normal Vectors and Offsetting

The normal vector (perpendicular to the tangent) is used to offset the curve for stroke width:

```janet
(defn normal-at [seg t]
  "Unit normal (perpendicular to tangent, pointing left) at parameter t."
  (let* ([tang (bezier-tangent seg t)]
         [dx (get tang 0)]
         [dy (get tang 1)]
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
```

### Fiber Rendering

The demo renders multiple parallel "fibers" (strokes) offset from the centerline:

```janet
(defn fiber-path-data [segments dist nsteps]
  "Generate SVG path data string for a single fiber."
  (var path-str "")
  (var first-point true)
  (def nseg (length segments))
  (def total-steps (* nseg nsteps))
  (var seg-idx 0)
  (each seg in segments
    (var i 0)
    (while (<= i nsteps)
      (let* ([t (/ (float i) (float nsteps))]
             [global-t (/ (float (+ (* seg-idx nsteps) i)) (float total-steps))]
             [tapered-dist (* dist (taper global-t))]
             [pt (offset-point seg t tapered-dist)])
        (if first-point
          (begin
            (set path-str (-> "M " (append (coord-str pt))))
            (set first-point false))
          (set path-str (-> path-str
                            (append " L ")
                            (append (coord-str pt))))))
      (set i (+ i 1)))
    (set seg-idx (+ seg-idx 1)))
  path-str)
```

Each fiber is sampled at `nsteps` points per segment, with the offset distance tapered at the endpoints for a natural appearance.

### The Elle Logo

The logo is a stylized "Y" shape with three strokes meeting at a junction:

```janet
(def junction [120.0 435.0])

(def diagonal @[
  {:p0 [390.0 54.0]
   :p1 [310.0 159.0]
   :p2 [210.0 319.0]
   :p3 [120.0 435.0]}
])

(def base @[
  (line->bezier [120.0 435.0] [420.0 432.0])
])

(def arm @[
  {:p0 [268.0 225.0]
   :p1 [240.0 182.0]
   :p2 [182.0 114.0]
   :p3 [108.0 59.0]}
])
```

### Fiber Configuration

```janet
(def num-fibers 7)
(def fiber-spread 66.0)    # total width of the fiber bundle
(def fiber-width 8.0)      # stroke width per fiber

(def fiber-colors @[
  "#f5d020"    # bright yellow
  "#e8a215"    # gold
  "#d47020"    # orange
  "#c43c1e"    # red-orange
  "#a01818"    # red
  "#551010"    # dark red
  "#1a1a1a"    # black
])
```

### SVG Assembly

The demo generates SVG `<path>` elements for each fiber and combines them into a complete SVG document:

```janet
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
```

## Sample Output

The demo generates an SVG file at `docs/logo.svg`. The output is a stylized "Y" shape with:
- A dominant diagonal stroke from top-right to the junction
- A horizontal base stroke extending right from the junction
- An arm stroke extending up-left from the junction
- Each stroke rendered as 7 parallel fibers with a yellow-to-black gradient

The SVG can be viewed in any web browser or image viewer.

## Elle Idioms Used

- **`defn`** — Function definition
- **`let*`** — Sequential bindings
- **`->`** — Thread-first macro for chaining operations
- **`var` / `set`** — Mutable variables (used for building strings)
- **`each`** — Iterate over a sequence
- **`while`** — Loop with mutable state
- **`begin`** — Sequence multiple expressions
- **Struct literals** — `{:p0 [...] :p1 [...] ...}` for Bézier segments
- **Array literals** — `@[...]` for mutable arrays
- **`spit`** — Write a string to a file

## Why This Demo?

This demo showcases:
1. **Vector math** — Core to graphics programming
2. **Bézier curves** — Industry-standard for smooth curves
3. **Functional composition** — Building complex shapes from simple operations
4. **String building** — Generating SVG markup
5. **File I/O** — Writing output to disk

It demonstrates that Elle can express graphics algorithms cleanly and idiomatically.

## Running the Demo

```bash
cargo run --release -- demos/logo.lisp
```

This generates `docs/logo.svg`. View it with:
```bash
# In a web browser
open docs/logo.svg

# Or with an image viewer
feh docs/logo.svg
imv docs/logo.svg
```

## Further Reading

- [SVG Path Specification](https://developer.mozilla.org/en-US/docs/Web/SVG/Tutorial/Paths)
- [Bézier Curves](https://en.wikipedia.org/wiki/B%C3%A9zier_curve)
- [Vector Math for Graphics](https://www.3dgep.com/vector-math/)
