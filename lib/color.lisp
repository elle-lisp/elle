(elle/epoch 7)
## lib/color.lisp — Color science library (pure Elle)
##
## Color space conversions, mixing, gradients, and perceptual distance.
## All colors are structs with :space and component fields.
##
## Usage:
##   (def color ((import "std/color")))
##   (color:rgb 0.5 0.3 0.8)                              => {:space :srgb :r 0.5 ...}
##   (color:convert (color:rgb 1.0 0.0 0.0) :lab)         => {:space :lab :l 53.2 ...}
##   (color:mix (color:rgb 1 0 0) (color:rgb 0 0 1) 0.5)  => blended color
##   (color:gradient (color:hsl 0 0.8 0.5) (color:hsl 240 0.8 0.5) 5)
##   (color:lighten (color:rgb 0.5 0 0) 0.2)
##   (color:distance c1 c2)                                => CIEDE2000 float

(fn []

  # ── Construction ───────────────────────────────────────────────────

  (defn rgb [r g b]
    {:space :srgb :r (float r) :g (float g) :b (float b)})

  (defn rgba [r g b a]
    {:space :srgb :r (float r) :g (float g) :b (float b) :a (float a)})

  (defn hsl [h s l]
    {:space :hsl :h (float h) :s (float s) :l (float l)})

  (defn lab [l a b]
    {:space :lab :l (float l) :a (float a) :b (float b)})

  (defn oklch [l c h]
    {:space :oklch :l (float l) :c (float c) :h (float h)})

  # ── sRGB ↔ Linear ─────────────────────────────────────────────────

  (defn srgb-to-linear [x]
    (if (<= x 0.04045)
      (/ x 12.92)
      (math/pow (/ (+ x 0.055) 1.055) 2.4)))

  (defn linear-to-srgb [x]
    (if (<= x 0.0031308)
      (* x 12.92)
      (- (* 1.055 (math/pow x (/ 1.0 2.4))) 0.055)))

  # ── sRGB ↔ HSL ────────────────────────────────────────────────────

  (defn rgb->hsl [c]
    (let* [r (get c :r) g (get c :g) b (get c :b)
           mx (max r (max g b))
           mn (min r (min g b))
           d  (- mx mn)
           l  (/ (+ mx mn) 2.0)
           s  (if (= d 0.0) 0.0
                 (/ d (- 1.0 (abs (- (* 2.0 l) 1.0)))))
           h  (if (= d 0.0) 0.0
                 (cond
                   ((= mx r) (* 60.0 (fmod (/ (- g b) d) 6.0)))
                   ((= mx g) (* 60.0 (+ (/ (- b r) d) 2.0)))
                   (true     (* 60.0 (+ (/ (- r g) d) 4.0)))))]
      {:space :hsl
       :h (if (< h 0.0) (+ h 360.0) h)
       :s (max 0.0 (min 1.0 s))
       :l (max 0.0 (min 1.0 l))}))

  (defn hsl->rgb [c]
    (let* [h (get c :h) s (get c :s) l (get c :l)
           ch  (* (- 1.0 (abs (- (* 2.0 l) 1.0))) s)
           hp  (/ (fmod h 360.0) 60.0)
           x   (* ch (- 1.0 (abs (- (fmod hp 2.0) 1.0))))
           [r1 g1 b1]
            (cond
              ((< hp 1.0) [ch x  0.0])
              ((< hp 2.0) [x  ch 0.0])
              ((< hp 3.0) [0.0 ch x])
              ((< hp 4.0) [0.0 x  ch])
              ((< hp 5.0) [x  0.0 ch])
              (true       [ch 0.0 x]))
           m (- l (/ ch 2.0))]
      {:space :srgb :r (+ r1 m) :g (+ g1 m) :b (+ b1 m)}))

  # ── sRGB ↔ XYZ (D65) ──────────────────────────────────────────────

  (defn rgb->xyz [c]
    (let* [rl (srgb-to-linear (get c :r))
           gl (srgb-to-linear (get c :g))
           bl (srgb-to-linear (get c :b))]
      {:space :xyz
       :x (+ (* 0.4124564 rl) (* 0.3575761 gl) (* 0.1804375 bl))
       :y (+ (* 0.2126729 rl) (* 0.7151522 gl) (* 0.0721750 bl))
       :z (+ (* 0.0193339 rl) (* 0.1191920 gl) (* 0.9503041 bl))}))

  (defn xyz->rgb [c]
    (let* [x (get c :x) y (get c :y) z (get c :z)
           rl (+ (* 3.2404542  x) (* -1.5371385 y) (* -0.4985314 z))
           gl (+ (* -0.9692660 x) (* 1.8760108  y) (* 0.0415560  z))
           bl (+ (* 0.0556434  x) (* -0.2040259 y) (* 1.0572252  z))]
      {:space :srgb
       :r (max 0.0 (min 1.0 (linear-to-srgb rl)))
       :g (max 0.0 (min 1.0 (linear-to-srgb gl)))
       :b (max 0.0 (min 1.0 (linear-to-srgb bl)))}))

  # ── XYZ ↔ Lab (D65) ───────────────────────────────────────────────

  (def D65-X 0.95047)
  (def D65-Y 1.00000)
  (def D65-Z 1.08883)
  (def LAB-E (/ 216.0 24389.0))
  (def LAB-K (/ 24389.0 27.0))

  (defn lab-f [t]
    (if (> t LAB-E)
      (math/cbrt t)
      (/ (+ (* LAB-K t) 16.0) 116.0)))

  (defn lab-f-inv [t]
    (let [t3 (* t t t)]
      (if (> t3 LAB-E) t3
        (/ (- (* 116.0 t) 16.0) LAB-K))))

  (defn xyz->lab [c]
    (let* [fx (lab-f (/ (get c :x) D65-X))
           fy (lab-f (/ (get c :y) D65-Y))
           fz (lab-f (/ (get c :z) D65-Z))]
      {:space :lab
       :l (- (* 116.0 fy) 16.0)
       :a (* 500.0 (- fx fy))
       :b (* 200.0 (- fy fz))}))

  (defn lab->xyz [c]
    (let* [l (get c :l) a (get c :a) b (get c :b)
           fy (/ (+ l 16.0) 116.0)
           fx (+ (/ a 500.0) fy)
           fz (- fy (/ b 200.0))]
      {:space :xyz
       :x (* D65-X (lab-f-inv fx))
       :y (* D65-Y (lab-f-inv fy))
       :z (* D65-Z (lab-f-inv fz))}))

  # ── sRGB ↔ Lab shortcut ───────────────────────────────────────────

  (defn rgb->lab [c] (xyz->lab (rgb->xyz c)))
  (defn lab->rgb [c] (xyz->rgb (lab->xyz c)))

  # ── Lab ↔ Oklch ────────────────────────────────────────────────────
  ## Oklch uses the Oklab perceptual space with cylindrical coordinates.
  ## sRGB → Linear → Oklab → Oklch

  (defn rgb->oklab [c]
    (let* [rl (srgb-to-linear (get c :r))
           gl (srgb-to-linear (get c :g))
           bl (srgb-to-linear (get c :b))
           l_ (+ (* 0.4122214708 rl) (* 0.5363325363 gl) (* 0.0514459929 bl))
           m_ (+ (* 0.2119034982 rl) (* 0.6806995451 gl) (* 0.1073969566 bl))
           s_ (+ (* 0.0883024619 rl) (* 0.2817188376 gl) (* 0.6299787005 bl))
           l (math/cbrt l_) m (math/cbrt m_) s (math/cbrt s_)]
      {:space :oklab
       :l (+ (* 0.2104542553 l) (* 0.7936177850 m) (* -0.0040720468 s))
       :a (+ (* 1.9779984951 l) (* -2.4285922050 m) (* 0.4505937099 s))
       :b (+ (* 0.0259040371 l) (* 0.7827717662 m) (* -0.8086757660 s))}))

  (defn oklab->rgb [c]
    (let* [L (get c :l) A (get c :a) B (get c :b)
           l_ (+ L (* 0.3963377774 A) (* 0.2158037573 B))
           m_ (+ L (* -0.1055613458 A) (* -0.0638541728 B))
           s_ (+ L (* -0.0894841775 A) (* -1.2914855480 B))
           l (* l_ l_ l_) m (* m_ m_ m_) s (* s_ s_ s_)
           rl (+ (* 4.0767416621 l) (* -3.3077115913 m) (* 0.2309699292 s))
           gl (+ (* -1.2684380046 l) (* 2.6097574011 m) (* -0.3413193965 s))
           bl (+ (* -0.0041960863 l) (* -0.7034186147 m) (* 1.7076147010 s))]
      {:space :srgb
       :r (max 0.0 (min 1.0 (linear-to-srgb rl)))
       :g (max 0.0 (min 1.0 (linear-to-srgb gl)))
       :b (max 0.0 (min 1.0 (linear-to-srgb bl)))}))

  (def DEG (/ 180.0 (math/pi)))
  (def RAD (/ (math/pi) 180.0))

  (defn oklab->oklch [c]
    (let* [a (get c :a) b (get c :b)
           ch (math/sqrt (+ (* a a) (* b b)))
           h  (* (math/atan2 b a) DEG)]
      {:space :oklch
       :l (get c :l)
       :c ch
       :h (if (< h 0.0) (+ h 360.0) h)}))

  (defn oklch->oklab [c]
    (let* [ch (get c :c) h (* (get c :h) RAD)]
      {:space :oklab
       :l (get c :l)
       :a (* ch (math/cos h))
       :b (* ch (math/sin h))}))

  (defn rgb->oklch [c] (oklab->oklch (rgb->oklab c)))
  (defn oklch->rgb [c] (oklab->rgb (oklch->oklab c)))

  # ── Conversion dispatch ───────────────────────────────────────────

  (defn to-srgb [c]
    (match (get c :space)
      (:srgb  c)
      (:hsl   (hsl->rgb c))
      (:lab   (lab->rgb c))
      (:xyz   (xyz->rgb c))
      (:oklab (oklab->rgb c))
      (:oklch (oklch->rgb c))
      (_      (error {:error :color-error :message (string "unknown space " (get c :space))}))))

  (defn convert [c space]
    (let [s (to-srgb c)]
      (match space
        (:srgb  s)
        (:hsl   (rgb->hsl s))
        (:lab   (rgb->lab s))
        (:xyz   (rgb->xyz s))
        (:oklab (rgb->oklab s))
        (:oklch (rgb->oklch s))
        (_      (error {:error :color-error :message (string "unknown target space " space)})))))

  # ── Operations ─────────────────────────────────────────────────────

  (defn mix [c1 c2 t]
    (let* [a (to-srgb c1) b (to-srgb c2)
           t (float t) inv (- 1.0 t)]
      {:space :srgb
       :r (+ (* (get a :r) inv) (* (get b :r) t))
       :g (+ (* (get a :g) inv) (* (get b :g) t))
       :b (+ (* (get a :b) inv) (* (get b :b) t))}))

  (defn gradient [c1 c2 n]
    (let [steps (max 2 n)]
      (map (fn [i] (mix c1 c2 (/ (float i) (float (dec steps)))))
           (range steps))))

  (defn lighten [c amount]
    (let [h (convert c :hsl)]
      (hsl->rgb (put h :l (max 0.0 (min 1.0 (+ (get h :l) (float amount))))))))

  (defn darken [c amount]
    (lighten c (- (float amount))))

  (defn saturate [c amount]
    (let [h (convert c :hsl)]
      (hsl->rgb (put h :s (max 0.0 (min 1.0 (+ (get h :s) (float amount))))))))

  (defn desaturate [c amount]
    (saturate c (- (float amount))))

  (defn complement [c]
    (let [h (convert c :hsl)]
      (hsl->rgb (put h :h (fmod (+ (get h :h) 180.0) 360.0)))))

  # ── CIEDE2000 color distance ───────────────────────────────────────

  (defn distance [c1 c2]
    (let* [a (convert c1 :lab) b (convert c2 :lab)
           l1 (get a :l) a1 (get a :a) b1 (get a :b)
           l2 (get b :l) a2 (get b :a) b2 (get b :b)
           dl (- l2 l1)
           lb (/ (+ l1 l2) 2.0)
           c1s (math/sqrt (+ (* a1 a1) (* b1 b1)))
           c2s (math/sqrt (+ (* a2 a2) (* b2 b2)))
           cb  (/ (+ c1s c2s) 2.0)
           cb7 (math/pow cb 7.0)
           g   (* 0.5 (- 1.0 (math/sqrt (/ cb7 (+ cb7 (math/pow 25.0 7.0))))))
           a1p (* a1 (+ 1.0 g))
           a2p (* a2 (+ 1.0 g))
           c1p (math/sqrt (+ (* a1p a1p) (* b1 b1)))
           c2p (math/sqrt (+ (* a2p a2p) (* b2 b2)))
           dcp (- c2p c1p)
           cbp (/ (+ c1p c2p) 2.0)
           h1p (let [h (* (math/atan2 b1 a1p) DEG)]
                  (if (< h 0.0) (+ h 360.0) h))
           h2p (let [h (* (math/atan2 b2 a2p) DEG)]
                  (if (< h 0.0) (+ h 360.0) h))
           dhp (cond
                  ((or (= c1p 0.0) (= c2p 0.0)) 0.0)
                  ((<= (abs (- h2p h1p)) 180.0) (- h2p h1p))
                  ((> (- h2p h1p) 180.0) (- (- h2p h1p) 360.0))
                  (true (+ (- h2p h1p) 360.0)))
           dHp (* 2.0 (math/sqrt (* c1p c2p)) (math/sin (* (/ dhp 2.0) RAD)))
           Hbp (cond
                  ((or (= c1p 0.0) (= c2p 0.0)) (+ h1p h2p))
                  ((<= (abs (- h1p h2p)) 180.0) (/ (+ h1p h2p) 2.0))
                  ((< (+ h1p h2p) 360.0) (/ (+ h1p h2p 360.0) 2.0))
                  (true (/ (+ h1p h2p -360.0) 2.0)))
           T (+ 1.0
                 (* -0.17 (math/cos (* (- Hbp 30.0) RAD)))
                 (* 0.24  (math/cos (* (* 2.0 Hbp) RAD)))
                 (* 0.32  (math/cos (* (+ (* 3.0 Hbp) 6.0) RAD)))
                 (* -0.20 (math/cos (* (- (* 4.0 Hbp) 63.0) RAD))))
           lbm50sq (* (- lb 50.0) (- lb 50.0))
           sl (+ 1.0 (/ (* 0.015 lbm50sq) (math/sqrt (+ 20.0 lbm50sq))))
           sc (+ 1.0 (* 0.045 cbp))
           sh (+ 1.0 (* 0.015 cbp T))
           cbp7 (math/pow cbp 7.0)
           rt (* -2.0 (math/sqrt (/ cbp7 (+ cbp7 (math/pow 25.0 7.0))))
                 (math/sin (* 60.0 (math/pow 2.718281828
                                     (* -1.0 (* (/ (- Hbp 275.0) 25.0) (/ (- Hbp 275.0) 25.0))))))
                 RAD)]
      (math/sqrt (+ (* (/ dl sl) (/ dl sl))
                    (* (/ dcp sc) (/ dcp sc))
                    (* (/ dHp sh) (/ dHp sh))
                    (* rt (/ dcp sc) (/ dHp sh))))))

  # ── Pixel interop ──────────────────────────────────────────────────

  (defn to-rgba8 [c]
    (let [s (to-srgb c)]
      [(integer (* (max 0.0 (min 1.0 (get s :r))) 255.0))
       (integer (* (max 0.0 (min 1.0 (get s :g))) 255.0))
       (integer (* (max 0.0 (min 1.0 (get s :b))) 255.0))
       (integer (* (max 0.0 (min 1.0 (or (get s :a) 1.0))) 255.0))]))

  (defn from-rgba8 [r g b a]
    {:space :srgb
     :r (/ (float r) 255.0)
     :g (/ (float g) 255.0)
     :b (/ (float b) 255.0)
     :a (/ (float a) 255.0)})

  # ── Export ─────────────────────────────────────────────────────────

  {:rgb rgb :rgba rgba :hsl hsl :lab lab :oklch oklch
   :convert convert :to-srgb to-srgb
   :mix mix :gradient gradient
   :lighten lighten :darken darken
   :saturate saturate :desaturate desaturate
   :complement complement
   :distance distance
   :to-rgba8 to-rgba8 :from-rgba8 from-rgba8})
