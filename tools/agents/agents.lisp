(elle/epoch 18)
# audited: 2026-11-23
# The index generator's command line as a function: it reads the arguments, writes or checks the indexes, and answers what to print.
# tools/agents/overview.md

(fn []
  (def {:tree tree :docs-in docs-in :under under} ((import "./listing")))
  (def {:index index :owned? owned?} ((import "./render")))
  (def {:counts counts} ((import "./counts")))

  (def usage
    "tools/agents/main.lisp — build AGENTS.md from every document in a directory

usage:
  main.lisp [DIR]         write the index for DIR and everything below it
  main.lisp --print DIR   write DIR's index to stdout
  main.lisp --check [DIR] exit 1 when a committed index is stale
  main.lisp --counts      the hand-written indexes, and the rows in them
                          that name a file
  main.lisp --root DIR    treat DIR as the repository root, which is the
                          working directory otherwise
  main.lisp --help        this text

An entry is a document's link, its `# Title` and its call-out — the first
sentence of the first paragraph under that title. A document with no call-out,
or one over the budget, is listed as uncalled, which is the work queue.

No run writes or counts the root AGENTS.md, the guide a person writes.
")

  (defn options [args]
    "Read ARGS into {:root :mode :dir}, or answer {:exit :stdout :stderr} for
     --help and for a usage error. A leading `--` is dropped."
    (let [given (->array args)
          @i (if (and (> (length given) 0) (= (get given 0) "--")) 1 0)
          @opts @{:root (path/cwd) :mode :write :dir "."}
          @done nil]
      (while (and (nil? done) (< i (length given)))
        (let [arg (get given i)]
          (cond
            (= arg "--root")
              (if (< (+ i 1) (length given))
                (begin
                  (put opts :root (get given (+ i 1)))
                  (assign i (+ i 1)))
                (assign done {:exit 2 :stdout "" :stderr usage}))
            (or (= arg "--help") (= arg "-h"))
              (assign done {:exit 0 :stdout usage :stderr ""})
            (contains? |"--print" "--check" "--counts"| arg)
              (put opts :mode (keyword (slice arg 2 (length arg))))
            (string/starts-with? arg "--")
              (assign done {:exit 2 :stdout "" :stderr usage})
            (put opts :dir arg)))
        (assign i (+ i 1)))
      (or done opts)))

  (defn normal [dir]
    "DIR relative to the root, with no leading `./` and no trailing slash."
    (let [d (if (string/starts-with? dir "./") (slice dir 2 (length dir)) dir)
          d (if (string/ends-with? d "/")
              (slice d 0 (- (length d) 1))
              d)]
      (if (empty? d) "." d)))

  (defn count-lines [root]
    "The two counts under ROOT, one line each."
    (let [c (counts root)]
      (string "hand-written " (get c :hand-written) "\nfile-rows "
              (get c :file-rows) "\n")))

  (defn targets [t dir]
    "Each directory a write or a check visits, with the path of its index: D
     and every directory below it that holds a document, the root aside."
    (map (fn [d] [d (path/join (get t :root) d "AGENTS.md")])
         (filter (fn [d] (and (not (= d ".")) (not (empty? (docs-in t d)))))
                 (under t dir))))

  (defn check [t dir]
    "Name each index the generator owns under DIR whose text is not the one it
     would write, and fail when there is one."
    (let [@err @[]]
      (each [d target] in (targets t dir)
        (when (and (owned? target)
                   (or (not (path/exists? target))
                       (not (= (file/read target) (index t d)))))
          (push err
                (string "AGENTS.md is stale in " d " — run: make agents\n"))))
      {:exit (if (empty? err) 0 1)
       :stdout (count-lines (get t :root))
       :stderr (string/join err "")}))

  (defn write [t dir]
    "Write each index the generator owns under DIR, and name each directory it
     declined because a person wrote its index."
    (let [@err @[]
          @n 0]
      (each [d target] in (targets t dir)
        (if (owned? target)
          (begin
            (file/write target (index t d))
            (assign n (+ n 1)))
          (push err (string "skipped " d ": hand-written AGENTS.md\n"))))
      {:exit 0
       :stdout (string "wrote " n " indexes\n" (count-lines (get t :root)))
       :stderr (string/join err "")}))

  (defn run [args]
    "Run the generator on ARGS, the arguments main.lisp takes, and answer
     {:exit :stdout :stderr}."
    (let [o (options args)]
      (if (get o :exit)
        o
        (let [root (get o :root)
              dir (normal (get o :dir))]
          (case (get o :mode)
            :counts {:exit 0 :stdout (count-lines root) :stderr ""}
            :print
              {:exit 0 :stdout (index (tree root) dir) :stderr ""}
            :check (check (tree root) dir)
            (write (tree root) dir))))))

  {:run run :counts counts})
