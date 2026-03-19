(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## clap plugin integration tests
## Tests clap/parse via the plugin .so loaded at runtime.
##
## Plugin functions are accessed through the struct returned by import-file,
## not as top-level globals, because file-as-letrec compiles the entire file
## before executing any of it — so top-level globals from the .so aren't
## available at compile time.

## Try to load the clap plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_clap.so")))
(when (not ok?)
  (display "SKIP: clap plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn (get plugin :parse))

# ── Happy paths ─────────────────────────────────────────────────────────────

## Simple long flag → true
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]} ["--verbose"])))
  (assert-true (get result :verbose) "long flag true"))

## Simple short flag → true
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :flag}]} ["-v"])))
  (assert-true (get result :verbose) "short flag true"))

## String option with long
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} ["--output" "foo"])))
  (assert-eq (get result :output) "foo" "string option long"))

## String option with short
(let ((result (parse-fn {:name "app" :args [{:name "output" :short "o"}]} ["-o" "foo"])))
  (assert-eq (get result :output) "foo" "string option short"))

## String option with = syntax
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} ["--output=foo"])))
  (assert-eq (get result :output) "foo" "string option with ="))

## Required positional arg
(let ((result (parse-fn {:name "app" :args [{:name "input" :required true}]} ["file.txt"])))
  (assert-eq (get result :input) "file.txt" "required positional"))

## Multiple positional args in order
(let ((result (parse-fn {:name "cp"
                          :args [{:name "src" :required true}
                                 {:name "dst" :required true}]}
                        ["foo.txt" "bar.txt"])))
  (assert-eq (get result :src) "foo.txt" "multi-positional src")
  (assert-eq (get result :dst) "bar.txt" "multi-positional dst"))

## Default value when arg absent
(let ((result (parse-fn {:name "app" :args [{:name "count" :long "count" :default "1"}]} [])))
  (assert-eq (get result :count) "1" "default value used"))

## Absent optional → nil (no default)
(let ((result (parse-fn {:name "app" :args [{:name "output" :long "output"}]} [])))
  (assert-true (nil? (get result :output)) "absent optional is nil"))

## Flag absent → false
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]} [])))
  (assert-false (get result :verbose) "flag absent is false"))

## Count flag -vvv → 3
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :count}]} ["-vvv"])))
  (assert-eq (get result :verbose) 3 "count flag three"))

## Count absent → 0
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :short "v" :action :count}]} [])))
  (assert-eq (get result :verbose) 0 "count absent is zero"))

## Append action: multiple -I → array
(let ((result (parse-fn {:name "app" :args [{:name "include" :short "I" :action :append}]}
                        ["-I" "/usr/include" "-I" "/opt/include"])))
  (let ((arr (get result :include)))
    (assert-eq (length arr) 2 "append length 2")
    (assert-eq (get arr 0) "/usr/include" "append first element")
    (assert-eq (get arr 1) "/opt/include" "append second element")))

## Append absent → empty array
(let ((result (parse-fn {:name "app" :args [{:name "include" :short "I" :action :append}]} [])))
  (assert-eq (length (get result :include)) 0 "append absent is empty array"))

## Subcommand matched
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build"
                                      :args [{:name "release" :long "release" :action :flag}]}
                                     {:name "test"
                                      :args [{:name "name" :long "name"}]}]}
                        ["build" "--release"])))
  (assert-eq (get result :command) "build" "subcommand name")
  (assert-true (get (get result :command-args) :release) "subcommand arg"))

## Subcommand with its own args, non-release
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build"
                                      :args [{:name "release" :long "release" :action :flag}]}
                                     {:name "test"
                                      :args [{:name "name" :long "name"}]}]}
                        ["test" "--name" "mytest"])))
  (assert-eq (get result :command) "test" "subcommand test name")
  (assert-eq (get (get result :command-args) :name) "mytest" "subcommand test arg"))

## No subcommand matched → :command nil, :command-args nil
(let ((result (parse-fn {:name "cargo"
                          :commands [{:name "build" :args []}]}
                        [])))
  (assert-true (nil? (get result :command)) "no subcommand :command nil")
  (assert-true (nil? (get result :command-args)) "no subcommand :command-args nil"))

## Mixed flags and positionals
(let ((result (parse-fn {:name "app"
                          :args [{:name "input" :required true}
                                 {:name "verbose" :long "verbose" :action :flag}]}
                        ["--verbose" "file.txt"])))
  (assert-eq (get result :input) "file.txt" "mixed positional")
  (assert-true (get result :verbose) "mixed flag"))

## Empty argv with no required args → all defaults/nils
(let ((result (parse-fn {:name "app"
                          :args [{:name "verbose" :long "verbose" :action :flag}
                                 {:name "output" :long "output"}]}
                        [])))
  (assert-false (get result :verbose) "empty argv flag false")
  (assert-true (nil? (get result :output)) "empty argv string nil"))

## List input for argv (not array)
(let ((result (parse-fn {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]}
                        (list "--verbose"))))
  (assert-true (get result :verbose) "list argv works"))

## Arg name with hyphens
(let ((result (parse-fn {:name "app" :args [{:name "dry-run" :long "dry-run" :action :flag}]}
                        ["--dry-run"])))
  (assert-true (get result :dry-run) "hyphenated arg name"))

## Empty :args array
(let ((result (parse-fn {:name "app" :args []} [])))
  (assert-not-nil result "empty args array returns struct"))

## Empty :commands array treated same as absent
(let ((result (parse-fn {:name "app" :args [{:name "v" :long "verbose" :action :flag}]
                          :commands []}
                        ["--verbose"])))
  (assert-true (get result :v) "empty commands array ignored"))

## :version in spec (just verify parsing still works; --version causes clap error, not crash)
(let ((result (parse-fn {:name "app" :version "1.0.0" :args [{:name "v" :long "verbose" :action :flag}]}
                        [])))
  (assert-false (get result :v) "spec with version parses ok"))

# ── Error paths ──────────────────────────────────────────────────────────────

## spec is not a struct → type-error
(assert-err-kind (fn () (parse-fn "not-a-struct" []))
  :type-error "spec not a struct")

## argv is not a list/array → type-error
(assert-err-kind (fn () (parse-fn {:name "app"} "not-a-list"))
  :type-error "argv not a list")

## argv element is not a string → type-error
(assert-err-kind (fn () (parse-fn {:name "app"} [42]))
  :type-error "argv element not a string")

## missing :name in spec → clap-error
(assert-err-kind (fn () (parse-fn {:about "no name"} []))
  :clap-error "spec missing :name")

## unknown flag in argv → clap-error
(assert-err-kind (fn () (parse-fn {:name "app" :args []} ["--unknown-flag"]))
  :clap-error "unknown flag in argv")

## missing required arg → clap-error
(assert-err-kind (fn () (parse-fn {:name "app" :args [{:name "input" :long "input" :required true}]} []))
  :clap-error "missing required arg")

## unknown :action keyword → clap-error
(assert-err-kind (fn () (parse-fn {:name "app" :args [{:name "x" :long "x" :action :bogus}]} []))
  :clap-error "unknown action keyword")

## :short with multi-char string → clap-error
(assert-err-kind (fn () (parse-fn {:name "app" :args [{:name "x" :short "ab"}]} []))
  :clap-error "short multi-char")

## arg spec missing :name → clap-error
(assert-err-kind (fn () (parse-fn {:name "app" :args [{:long "x"}]} []))
  :clap-error "arg missing :name")

## arg named "command" with :commands present → clap-error
(assert-err-kind
  (fn () (parse-fn {:name "app"
                     :args [{:name "command" :long "command"}]
                     :commands [{:name "sub"}]}
                   []))
  :clap-error "reserved arg name command")

## arg named "command-args" with :commands present → clap-error
(assert-err-kind
  (fn () (parse-fn {:name "app"
                     :args [{:name "command-args" :long "command-args"}]
                     :commands [{:name "sub"}]}
                   []))
  :clap-error "reserved arg name command-args")
