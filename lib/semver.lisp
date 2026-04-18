(elle/epoch 8)
## lib/semver.lisp — Semantic versioning (pure Elle)
##
## Implements semver 2.0.0 parsing, comparison, and requirement matching.
##
## Usage:
##   (def semver ((import "std/semver")))
##   (semver:parse "1.2.3")            => {:major 1 :minor 2 :patch 3 :pre "" :build ""}
##   (semver:valid? "1.2.3")           => true
##   (semver:compare "1.0.0" "2.0.0")  => -1
##   (semver:satisfies? "1.2.3" ">=1") => true
##   (semver:increment "1.2.3" :minor) => "1.3.0"

(fn []

  (defn parse-int-strict [s ctx]
    "Parse s as a non-negative integer with no leading zeros."
    (when (empty? s)
      (error {:error :semver-error :message (string ctx ": empty version component")}))
    (when (and (> (length s) 1) (string/starts-with? s "0"))
      (error {:error :semver-error :message (string ctx ": leading zero in \"" s "\"")}))
    (let [n (parse-int s)]
      (when (nil? n)
        (error {:error :semver-error :message (string ctx ": non-numeric \"" s "\"")}))
      (when (< n 0)
        (error {:error :semver-error :message (string ctx ": negative \"" s "\"")}))
      n))

  (defn parse [version]
    "Parse a semver string into {:major :minor :patch :pre :build}."
    (unless (string? version)
      (error {:error :type-error
              :message (string "semver/parse: expected string, got " (type-of version))}))
    (let* [plus-parts  (string/split version "+")
           build-str   (if (> (length plus-parts) 1)
                          (string/join (rest plus-parts) "+") "")
           dash-parts  (string/split (first plus-parts) "-")
           pre-str     (if (> (length dash-parts) 1)
                          (string/join (rest dash-parts) "-") "")
           parts       (string/split (first dash-parts) ".")]
      (unless (= (length parts) 3)
        (error {:error :semver-error
                :message (string "semver/parse: invalid version \"" version "\"")}))
      {:major (parse-int-strict (parts 0) "semver/parse")
       :minor (parse-int-strict (parts 1) "semver/parse")
       :patch (parse-int-strict (parts 2) "semver/parse")
       :pre pre-str :build build-str}))

  (defn valid? [version]
    "Check if a string is a valid semver version."
    (unless (string? version)
      (error {:error :type-error
              :message (string "semver/valid?: expected string, got " (type-of version))}))
    (first (protect ((fn [] (parse version) true)))))

  (defn compare-pre [a b]
    "Compare pre-release strings per semver 2.0.0 spec §11."
    (if (= a b) 0
      (if (= a "") 1          # no pre-release > any pre-release
        (if (= b "") -1
          (let [as (string/split a ".")
                bs (string/split b ".")]
            (defn cmp-ids [ai bi]
              (if (and (>= ai (length as)) (>= bi (length bs))) 0
                (if (>= ai (length as)) -1
                  (if (>= bi (length bs)) 1
                    (let* [av (as ai) bv (bs bi)
                           an (parse-int av) bn (parse-int bv)
                           r (match [an bn]
                                [[nil nil] (compare av bv)]
                                [[nil _]   1]
                                [[_ nil]  -1]
                                [_  (compare an bn)])]
                      (if (zero? r) (cmp-ids (inc ai) (inc bi)) r))))))
            (cmp-ids 0 0))))))

  (defn semver-compare [a b]
    "Compare two semver version strings. Returns -1, 0, or 1."
    (let [va (parse a) vb (parse b)]
      (defn cmp-fields [fields]
        (if (empty? fields) (compare-pre va:pre vb:pre)
          (let* [f (first fields) d (compare (va f) (vb f))]
            (if (zero? d) (cmp-fields (rest fields)) d))))
      (cmp-fields (list :major :minor :patch))))

  (defn parse-op [s]
    "Parse operator prefix from a requirement string. Returns [ver-str op]."
    (let [len (length s)]
      (if (string/starts-with? s ">=") [(string/trim (slice s 2 len)) ">="]
        (if (string/starts-with? s "<=") [(string/trim (slice s 2 len)) "<="]
          (if (string/starts-with? s "!=") [(string/trim (slice s 2 len)) "!="]
            (if (string/starts-with? s ">")  [(string/trim (slice s 1 len)) ">"]
              (if (string/starts-with? s "<")  [(string/trim (slice s 1 len)) "<"]
                (if (string/starts-with? s "=")  [(string/trim (slice s 1 len)) "="]
                  (if (string/starts-with? s "^")  [(string/trim (slice s 1 len)) "^"]
                    (if (string/starts-with? s "~")  [(string/trim (slice s 1 len)) "~"]
                      [s "^"])))))))))
)
  (defn satisfies-one? [ver req-str]
    "Check if version string ver satisfies a single requirement."
    (let* [trimmed (string/trim req-str)
           [ver-str op] (parse-op trimmed)
           c (semver-compare ver ver-str)]
      (match op
        [">=" (>= c 0)]
        ["<=" (<= c 0)]
        [">"  (> c 0)]
        ["<"  (< c 0)]
        ["="  (zero? c)]
        ["!=" (not (zero? c))]
        ["^"  (let [rv (parse ver-str) pv (parse ver)]
                (and (>= c 0)
                     (if (> rv:major 0)
                       (= pv:major rv:major)
                       (if (> rv:minor 0)
                         (and (zero? pv:major) (= pv:minor rv:minor))
                         (and (zero? pv:major) (zero? pv:minor)
                              (<= pv:patch rv:patch))))))]
        ["~"  (let [rv (parse ver-str) pv (parse ver)]
                (and (>= c 0)
                     (= pv:major rv:major)
                     (= pv:minor rv:minor)))]
        [_    (error {:error :semver-error
                      :message (string "semver/satisfies?: unknown operator " op)})])))

  (defn satisfies? [version requirement]
    "Check if version satisfies a requirement string (comma-separated)."
    (all? (fn [r] (satisfies-one? version (string/trim r)))
          (string/split requirement ",")))

  (defn increment [version part]
    "Increment a version part (:major, :minor, or :patch). Clears pre/build."
    (let [v (parse version)]
      (match part
        [:major (string (inc v:major) ".0.0")]
        [:minor (string v:major "." (inc v:minor) ".0")]
        [:patch (string v:major "." v:minor "." (inc v:patch))]
        [_ (error {:error :semver-error
                   :message (string "semver/increment: expected :major, :minor, or :patch, got " part)})])))

  {:parse parse :valid? valid? :compare semver-compare
   :satisfies? satisfies? :increment increment})
