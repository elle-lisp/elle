(elle/epoch 18)
# audited: 2026-11-23
# Lists the files an index may name under a root, and the directories that hold them.
# tools/agents/overview.md
#
# A directory is its path relative to the root, and the root itself is ".".

(fn []
  (defn lines [text]
    "The non-empty lines of TEXT."
    (filter (fn [l] (not (empty? l))) (->array (string/split text "\n"))))

  (defn top? [root]
    "Whether ROOT is the top of a git work tree. A root below one, or with no
     repository at all, is not."
    (let [[ok? r] (protect (subprocess/system "git"
                             ["-C" root "rev-parse" "--show-cdup"]))]
      (and ok? (= (get r :exit) 0) (empty? (string/trim (get r :stdout))))))

  (defn tracked [root]
    "The paths git tracks under ROOT."
    (let [r (subprocess/system "git"
                               ["-C" root "-c" "core.quotePath=false" "ls-files"])]
      (unless (= (get r :exit) 0)
        (error {:error :agents
                :message (string "git ls-files failed in " root ": "
                                 (get r :stderr))}))
      (lines (get r :stdout))))

  (defn kind [path]
    "The type of the entry at PATH, not following a symbolic link."
    (get (file/lstat path) :file-type))

  (defn walked [root]
    "Every regular file under ROOT. The walk skips `target` and `node_modules`
     by name, and enters no directory below ROOT that holds a `.git` of its
     own, which is another repository."
    (let [@out @[]]
      (letrec [visit (fn [rel]
                       (each name in (file/ls
                                       (if (= rel ".") root (path/join root rel)))
                         (let [sub (if (= rel ".") name (string rel "/" name))
                               full (path/join root sub)
                               t (kind full)]
                           (cond
                             (= t "file") (push out sub)
                             (and (= t "dir")
                                  (not (contains? |"target" "node_modules"| name))
                                  (not (path/exists? (path/join full ".git"))))
                               (visit sub)
                             nil))))]
        (visit "."))
      out))

  (defn hidden? [path]
    "Whether a component of PATH starts with a dot."
    (any? (fn [c] (string/starts-with? c ".")) (->array (string/split path "/"))))

  (defn listing [root]
    "Every file an index under ROOT may name, sorted: what git tracks when ROOT
     is the top of a work tree, and every file otherwise. Each is a file on
     disk, and no component of its path starts with a dot."
    (let [paths (if (top? root) (tracked root) (walked root))]
      (sort (->array (filter (fn [p]
                               (and (not (hidden? p))
                                    (path/file? (path/join root p)))) paths)))))

  (defn parent-of [path]
    "The directory that holds PATH."
    (let [parts (->array (string/split path "/"))
          n (length parts)]
      (if (= n 1)
        "."
        (string/join (slice parts 0 (- n 1)) "/"))))

  (defn name-of [path]
    "The last component of PATH."
    (last (->array (string/split path "/"))))

  (defn tree [root]
    "The directories of ROOT's listing: {:root :dirs}, where :dirs maps each
     directory to {:files :subdirs}, two @sets of names."
    (let [@dirs @{}]
      (letrec [entry (fn [d]
                       (or (get dirs d)
                           (let [e {:files @|| :subdirs @||}]
                             (put dirs d e)
                             (unless (= d ".")
                               (add (get (entry (parent-of d)) :subdirs)
                                    (name-of d)))
                             e)))]
        (each path in (listing root)
          (add (get (entry (parent-of path)) :files) (name-of path))))
      {:root root :dirs dirs}))

  (defn path-in [d name]
    "The path of NAME inside directory D."
    (if (= d ".") name (string d "/" name)))

  (defn names [t d key]
    "The sorted names under KEY of directory D in the tree T."
    (let [e (get (get t :dirs) d)]
      (if e
        (sort (->array (get e key)))
        [])))

  (defn holds? [t d name]
    "Whether directory D of the tree T holds the file NAME."
    (let [e (get (get t :dirs) d)]
      (and e (contains? (get e :files) name))))

  (defn docs-in [t d]
    "The documents directly in D, the index and CLAUDE.md aside."
    (map (fn [n] (path-in d n))
         (filter (fn [n]
                   (and (string/ends-with? n ".md")
                        (not (contains? |"AGENTS.md" "CLAUDE.md"| n))))
                 (names t d :files))))

  (defn indexed? [t d]
    "Whether D has an index to link: a document of its own, or an AGENTS.md
     not yet converted."
    (or (not (empty? (docs-in t d))) (holds? t d "AGENTS.md")))

  (defn subdirs-in [t d]
    "The subdirectories of D that have an index to link."
    (filter (fn [s] (indexed? t s))
            (map (fn [n] (path-in d n)) (names t d :subdirs))))

  (defn under [t d]
    "D, then every directory of the tree T below it, sorted."
    (let [prefix (if (= d ".") "" (string d "/"))]
      (concat [d]
              (sort (->array (filter (fn [k]
                                       (and (not (= k "."))
                                         (string/starts-with? k prefix)))
                                     (keys (get t :dirs))))))))

  {:listing listing
   :tree tree
   :docs-in docs-in
   :subdirs-in subdirs-in
   :under under
   :name-of name-of})
