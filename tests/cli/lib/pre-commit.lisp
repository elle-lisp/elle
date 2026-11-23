(elle/epoch 18)
## audited: 2026-11-22
## A scratch repository that runs the real commit hook against the binary under test, and what each commit left in it.
## tests/cli/overview.md
## CONTRIBUTING.md
##
## Splice this after the runner's harness, whose elle-feed and env-without it
## reads. The audit gate and the index generator are stubbed: their own tests
## live elsewhere. The stub generator copies AGENTS.next over AGENTS.md when a
## scenario has written one, which stands in for an index it rewrote.
##
## The trap every scenario guards: `git commit -- PATHS` builds a temporary
## index and runs the hook against it. A `git add` in the hook reaches only that
## index, so the commit takes the formatted file while the real index keeps the
## unformatted one. `git status` then shows `MM`, and the next commit taken from
## the index silently reverts the formatting.

(def hook-source (file/read ".githooks/pre-commit"))

# A formatted body, the same body unformatted, and a second formatted body.
(def tidy "(defn f [x]\n  (+ x 1))\n")
(def untidy "(defn f [x]     (+ x 1))\n")
(def other "(defn g [y]\n  (* y 2))\n")

# Epoch 10 renamed `cons` to `pair`, and the hook runs `elle fmt` without
# `--no-epoch`, so it migrates this file.
(def old-epoch "(elle/epoch 9)\n(def p (cons 1 2))\n")

(defn formatted [source]
  "SOURCE as `elle fmt` writes it, at the current epoch."
  (let [r (elle-feed ["fmt"] source)]
    (assert (= r:exit 0) (string "elle fmt failed on " source ": " r:err))
    r:out))

(defn git [repo args]
  "Git in REPO, isolated from the caller's configuration and from any
   repository the test itself runs inside."
  (subprocess/system "git" args {:env repo:env :cwd repo:dir}))

(defn must [repo args]
  "Git in REPO, which must succeed. Answer its stdout."
  (let [r (git repo args)]
    (assert (= r:exit 0) (string "git " args " failed: " r:stdout r:stderr))
    r:stdout))

(defn transcript [r]
  "What git run R wrote. A commit reports nothing to commit on stdout, and the
   hook reports a refusal on stderr."
  (string r:stdout r:stderr))

(defn repo-write [repo rel body]
  (let [path (path/join repo:dir rel)]
    (file/mkdir-all (path/parent path))
    (file/write path body)
    path))

(defn repo-read [repo rel]
  (file/read (path/join repo:dir rel)))

(defn repo-script [repo rel body]
  (subprocess/system "chmod" ["755" (repo-write repo rel body)]))

(defn repo-seed [repo rel body]
  "Commit BODY as REL without the hook. A partial commit names only tracked
   paths, so a scenario seeds the file it later changes."
  (repo-write repo rel body)
  (must repo ["add" rel])
  (must repo ["commit" "-q" "--no-verify" "-m" "seed"]))

(defn repo-head [repo rel]
  "REL as HEAD holds it."
  (must repo ["show" (string "HEAD:" rel)]))

(defn repo-commit [repo extra]
  "Commit with the hook, and answer what git left."
  (git repo ["commit" "-q" "-m" "change" ;extra]))

(defn at-head [repo]
  "Require that nothing staged differs from HEAD. A real index behind HEAD is
   exactly a staged difference."
  (let [staged (must repo ["diff" "--cached" "--name-only" "HEAD"])]
    (assert (empty? staged)
            (string "the real index differs from HEAD in: " staged "; status: "
                    (must repo ["status" "--short"])))))

(defn scratch-repo [root name]
  "A repository named NAME under ROOT, holding the hook, its stubs and one
   commit."
  (let [dir (path/join root name)
        env (-> (env-without "GIT_DIR" "GIT_INDEX_FILE" "GIT_WORK_TREE")
                (put "ELLE" elle)
                (put "GIT_CONFIG_GLOBAL" "/dev/null")
                (put "GIT_CONFIG_NOSYSTEM" "1")
                (put "CARGO_TARGET_DIR" (path/join dir "target")))
        repo {:dir dir :env env}]
    (file/mkdir-all dir)
    (must repo ["init" "-q" "-b" "main"])
    (must repo ["config" "user.name" "Test"])
    (must repo ["config" "user.email" "test@example.invalid"])
    (must repo ["config" "commit.gpgsign" "false"])
    (must repo ["config" "core.hooksPath" ".githooks"])
    (repo-script repo ".githooks/pre-commit" hook-source)
    (repo-script repo "scripts/audit" "#!/bin/sh\nexit 0\n")
    (repo-script repo "scripts/agents"
                 "#!/bin/sh\n[ -f AGENTS.next ] && cp AGENTS.next AGENTS.md\nexit 0\n")
    (repo-write repo ".gitignore" "target/\nAGENTS.next\n")
    (must repo ["add" "."])
    (must repo ["commit" "-q" "--no-verify" "-m" "scaffold"])
    repo))
