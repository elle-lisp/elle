(elle/epoch 6)

## clap plugin integration tests
## Tests clap/parse via the plugin .so loaded at runtime.
##
## Plugin functions are accessed through the struct returned by import-file,
## not as top-level globals, because file-as-letrec compiles the entire file
## before executing any of it — so top-level globals from the .so aren't
## available at compile time.

## Try to load the clap plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-native "clap")))
(when (not ok?)
  (print "SKIP: clap plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn (get plugin :parse))

# ── Happy paths ─────────────────────────────────────────────────────────────

## Simple long flag → true
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]} ["myapp" "--verbose"])))
  (assert (get result :verbose) "long flag true"))

## Simple short flag → true
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :flag}]} ["myapp" "-v"])))
  (assert (get result :verbose) "short flag true"))

## String option with long
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} ["myapp" "--output" "foo"])))
  (assert (= (get result :output) "foo") "string option long"))

## String option with short
(let ((result (parse-fn {:name "app" :args [{:name "output" :short "o"}]} ["myapp" "-o" "foo"])))
  (assert (= (get result :output) "foo") "string option short"))

## String option with = syntax
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} ["myapp" "--output=foo"])))
  (assert (= (get result :output) "foo") "string option with ="))

## Required positional arg
(let ((result (parse-fn {:name "app" :args [{:name "input" :required true}]} ["myapp" "file.txt"])))
  (assert (= (get result :input) "file.txt") "required positional"))

## Multiple positional args in order
(let ((result (parse-fn {:name "cp"
                          :args [{:name "src" :required true}
                                 {:name "dst" :required true}]}
                        ["myapp" "foo.txt" "bar.txt"])))
  (assert (= (get result :src) "foo.txt") "multi-positional src")
  (assert (= (get result :dst) "bar.txt") "multi-positional dst"))

## Default value when arg absent
(let ((result (parse-fn {:name "app" :args [{:name "count" :long "count" :default "1"}]} ["myapp"])))
  (assert (= (get result :count) "1") "default value used"))

## Absent optional → nil (no default)
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} ["myapp"])))
  (assert (nil? (get result :output)) "absent optional is nil"))

## Flag absent → false
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]} ["myapp"])))
  (assert (not (get result :verbose)) "flag absent is false"))

## Count flag -vvv → 3
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :count}]} ["myapp" "-vvv"])))
  (assert (= (get result :verbose) 3) "count flag three"))

## Count absent → 0
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :count}]} ["myapp"])))
  (assert (= (get result :verbose) 0) "count absent is zero"))

## Append action: multiple -I → array
(let ((result (parse-fn {:name "app" :args [{:name "include" :short "I" :action :append}]}
                        ["myapp" "-I" "/usr/include" "-I" "/opt/include"])))
  (let ((arr (get result :include)))
    (assert (= (length arr) 2) "append length 2")
    (assert (= (get arr 0) "/usr/include") "append first element")
    (assert (= (get arr 1) "/opt/include") "append second element")))

## Append absent → empty array
(let ((result (parse-fn {:name "app" :args [{:name "include" :short "I" :action :append}]} ["myapp"])))
  (assert (= (length (get result :include)) 0) "append absent is empty array"))

## Subcommand matched
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build"
                                      :args [{:name "release" :long "release" :action :flag}]}
                                     {:name "test"
                                      :args [{:name "name" :long "name"}]}]}
                        ["myapp" "build" "--release"])))
  (assert (= (get result :command) "build") "subcommand name")
  (assert (get (get result :command-args) :release) "subcommand arg"))

## Subcommand with its own args, non-release
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build"
                                      :args [{:name "release" :long "release" :action :flag}]}
                                     {:name "test"
                                      :args [{:name "name" :long "name"}]}]}
                        ["myapp" "test" "--name" "mytest"])))
  (assert (= (get result :command) "test") "subcommand test name")
  (assert (= (get (get result :command-args) :name) "mytest") "subcommand test arg"))

## No subcommand matched → :command nil, :command-args nil
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build" :args []}]}
                        ["myapp"])))
  (assert (nil? (get result :command)) "no subcommand :command nil")
  (assert (nil? (get result :command-args)) "no subcommand :command-args nil"))

## Mixed flags and positionals
(let ((result (parse-fn {:name "app"
                          :args [{:name "input" :required true}
                                 {:name "verbose" :long "verbose" :action :flag}]}
                        ["myapp" "--verbose" "file.txt"])))
  (assert (= (get result :input) "file.txt") "mixed positional")
  (assert (get result :verbose) "mixed flag"))

## Empty argv with no required args → all defaults/nils
(let ((result (parse-fn {:name "app"
                          :args [{:name "verbose" :long "verbose" :action :flag}
                                 {:name "output" :long "output"}]}
                        ["myapp"])))
  (assert (not (get result :verbose)) "empty argv flag false")
  (assert (nil? (get result :output)) "empty argv string nil"))

## List input for argv (not array)
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]}
                        (list "myapp" "--verbose"))))
  (assert (get result :verbose) "list argv works"))

## Arg name with hyphens
(let ((result (parse-fn {:name "app" :args [{:name "dry-run" :long "dry-run" :action :flag}]}
                        ["myapp" "--dry-run"])))
  (assert (get result :dry-run) "hyphenated arg name"))

## Empty :args array
(let ((result (parse-fn {:name "app" :args []} ["myapp"])))
  (assert (not (nil? result)) "empty args array returns struct"))

## Empty :commands array treated same as absent
(let ((result (parse-fn {:name "app" :args [{:name "v" :long "verbose" :action :flag}]
                          :commands []}
                        ["myapp" "--verbose"])))
  (assert (get result :v) "empty commands array ignored"))

## :version in spec (just verify parsing still works; --version causes clap error, not crash)
(let ((result (parse-fn {:name "app" :version "1.0.0" :args [{:name "v" :long "verbose" :action :flag}]}
                        ["myapp"])))
  (assert (not (get result :v)) "spec with version parses ok"))

# ── Error paths ──────────────────────────────────────────────────────────────

## spec is not a struct → type-error
(let (([ok? err] (protect ((fn () (parse-fn "not-a-struct" [])))))) (assert (not ok?) "spec not a struct") (assert (= (get err :error) :type-error) "spec not a struct"))

## argv is not a list/array → type-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app"} "not-a-list")))))) (assert (not ok?) "argv not a list") (assert (= (get err :error) :type-error) "argv not a list"))

## argv element is not a string → type-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app"} [42])))))) (assert (not ok?) "argv element not a string") (assert (= (get err :error) :type-error) "argv element not a string"))

## missing :name in spec → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:about "no name"} [])))))) (assert (not ok?) "spec missing :name") (assert (= (get err :error) :clap-error) "spec missing :name"))

## unknown flag in argv → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app" :args []} ["myapp" "--unknown-flag"])))))) (assert (not ok?) "unknown flag in argv") (assert (= (get err :error) :clap-error) "unknown flag in argv"))

## missing required arg → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app" :args [{:name "input" :long "input" :required true}]} ["myapp"])))))) (assert (not ok?) "missing required arg") (assert (= (get err :error) :clap-error) "missing required arg"))

## unknown :action keyword → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app" :args [{:name "x" :long "x" :action :bogus}]} [])))))) (assert (not ok?) "unknown action keyword") (assert (= (get err :error) :clap-error) "unknown action keyword"))

## :short with multi-char string → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app" :args [{:name "x" :short "ab"}]} [])))))) (assert (not ok?) "short multi-char") (assert (= (get err :error) :clap-error) "short multi-char"))

## arg spec missing :name → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app" :args [{:long "x"}]} [])))))) (assert (not ok?) "arg missing :name") (assert (= (get err :error) :clap-error) "arg missing :name"))

## arg named "command" with :commands present → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app"
                     :args [{:name "command" :long "command"}]
                     :commands [{:name "sub"}]}
                   [])))))) (assert (not ok?) "reserved arg name command") (assert (= (get err :error) :clap-error) "reserved arg name command"))

## arg named "command-args" with :commands present → clap-error
(let (([ok? err] (protect ((fn () (parse-fn {:name "app"
                     :args [{:name "command-args" :long "command-args"}]
                     :commands [{:name "sub"}]}
                   [])))))) (assert (not ok?) "reserved arg name command-args") (assert (= (get err :error) :clap-error) "reserved arg name command-args"))
