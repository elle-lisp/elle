(elle/epoch 18)
## audited: 2026-11-25
## An image boot compiles a program to a source boot's bytecode, and the parity check never compares two source boots.
## tests/cli/overview.md
## docs/impl/image/boot.md
## docs/impl/image/pins.md

(include-file "../runner/lib/harness.lisp")

(def golden (path/join (path/cwd) "scripts" "bytecode-golden"))

(defn executable [dir name body]
  "Write BODY as the shell script NAME under DIR, make it executable, and
   answer its path."
  (let [path (write-fixture dir name body)]
    (subprocess/system "chmod" ["755" path])
    path))

(defn first-difference [a b]
  "The first line, numbered from one, where texts A and B part, with each
   side's line there, or nil when they agree. A dump is thousands of lines,
   so a failure names the place rather than printing both."
  (let [left (string/split a "\n")
        right (string/split b "\n")]
    (def @n 0)
    (def @found nil)
    (while (and (nil? found) (< n (max (length left) (length right))))
      (let [l (get left n "<end>")
            r (get right n "<end>")]
        (unless (= l r)
          (assign found [(+ n 1) l r])))
      (assign n (+ n 1)))
    found))

(defn boot-image-of [argv]
  "The last `--boot-image=` value ARGV names, which is the one `elle` keeps,
   or nil when it names none."
  (let [prefix "--boot-image="
        named (filter (fn [a] (string/starts-with? a prefix))
                      (string/split argv " "))]
    (if (empty? named) nil (slice (last named) (length prefix)))))

(with-temp-dir dir
  # Each boot runs in a process of its own: the static-region counter is
  # process-global, so two boots in one process would share it. The
  # counter-factual is a hydration that leaves that counter where a fresh
  # process starts it. The program compiles, fuses and runs as before, and
  # every region slot in its bytecode is lower than the source boot's, which
  # only a comparison of the bytecode can see.
  (eprintln "scenario: an image boot compiles a program to the bytecode a source boot does")
  (let [file (write-fixture dir "parity.lisp"
                            (string "(defn grow [xs] (map inc xs))\n"
                                    "(defn add-three [] (push [1 2] 3))\n"
                                    "(defn quokka-of [m] m:quokka)\n"
                                    "(quokka-of {})\n"))
        image (string "--boot-image=" (path/join dir "boot"))
        stored (elle-run ["--cache=" image "-e" "(+ 1 2)"])]
    (assert (= stored:exit 0) (string "the storing start failed: " stored:err))
    # Every dump turns the boot image off first, so the source dump compiles
    # whatever the binary carries, and the image dump's flag overrides it.
    (defn dump [flags]
      (let [r (elle-run ["--cache=" "--boot-image=off" ;flags "--dump=bytecode"
                         file])]
        (assert (= r:exit 0)
                (string "elle " flags " --dump=bytecode failed: " r:err))
        r))
    (let [source (dump [])
          hydrated (dump [image "--trace=boot"])]
      (assert (any? (fn [l]
                      (and (string/starts-with? l "[trace:boot]")
                           (string/contains? l "image-hydrate")))
                    (lines hydrated:err))
              (string "the image dump booted from source, so the comparison "
                      "is between two source boots: " hydrated:err))
      (assert (string/contains? source:out "region=")
              (string "the source dump names no region slot, so a counter "
                      "that never moved would pass: " source:out))
      (let [d (first-difference source:out hydrated:out)]
        (assert (nil? d)
                (string "an image boot compiles the fixture to other bytecode, "
                        "first at line " (get d 0) ": source boot " (get d 1)
                        "; image boot " (get d 2))))))

  # Two source boots always agree, so the check refuses to dump anything until
  # a start proves it hydrated the image it stored. The binary here reads every
  # `--boot-image=` it is handed as `off`: it accepts the flag and boots from
  # source, which a check without the proof would pass on every file. The trap:
  # a wrapper that dropped the flag would leave a release binary to hydrate its
  # embedded blob, and the proof would hold.
  (eprintln "scenario: the parity check refuses to compare two source boots")
  (let [deaf (executable dir "deaf-elle"
                         (string "#!/bin/sh\n" "for a do shift; case \"$a\" in "
                                 "--boot-image=*) set -- \"$@\" --boot-image=off ;; "
                                 "*) set -- \"$@\" \"$a\" ;; esac; done\n"
                                 "exec '" elle "' \"$@\"\n"))
        out (path/join dir "refused")
        r (subprocess/system golden ["parity" deaf out "1"])]
    (assert (not (= r:exit 0))
            (string "the parity check passed over a binary that never hydrates: "
                    r:stderr))
    (assert (string/contains? r:stderr "did not hydrate")
            (string "the parity check failed without saying the image never "
                    "hydrated: " r:stderr))
    (assert (not (file-exists? (path/join out "source")))
            "the parity check dumped files before it proved the image hydrated"))

  # A binary that carries a blob hydrates it under `--cache=` alone, so a
  # source side that named no policy would compare the blob with the warm
  # cache: two image boots, which agree on exactly the counter parity exists
  # to catch. The binary here records each argv and answers as a hydrating
  # `elle` does. The trap: the whole corpus is about 10,000 process starts,
  # which a macOS runner took past the runner's 60 s budget, so the check
  # dumps three files the scenario names.
  (eprintln "scenario: the parity check's source boots turn the boot image off")
  (let [log (path/join dir "argv.log")
        fake (executable dir "recording-elle"
                         (string "#!/bin/sh\n" "echo \"$*\" >> '" log "'\n"
                                 "case \" $* \" in *' --trace=boot '*) "
                                 "echo '[trace:boot] image-hydrate 0.1ms' >&2 ;; esac\n"
                                 "case \" $* \" in *' --dump=bytecode '*) "
                                 "echo 'one dump' ;; esac\n" "exit 0\n"))
        files (map (fn [n] (write-fixture dir n "(+ 1 2)\n"))
                   ["one.lisp" "two.lisp" "three.lisp"])
        out (path/join dir "parity")
        r (subprocess/system golden ["parity" fake out "8" ;files])
        image (string out "/boot")]
    (assert (= r:exit 0)
            (string "the parity check failed over a binary that answers alike: "
                    r:stderr))
    (let [dumps (filter (fn [a] (string/contains? a "--dump=bytecode"))
                        (lines (file/read log)))
          dumped (fn [policy]
                   (sort (map (fn [a] (last (string/split a " ")))
                              (filter (fn [a] (= (boot-image-of a) policy))
                                      dumps))))]
      # The counter-factual: a script that ignored the names would dump the
      # corpus, and the counts alone would read only how big it is.
      (assert (= (length dumps) (* 2 (length files)))
              (string "one dump per named file under each boot: " (length dumps)
                      " dumps of " (length files) " files"))
      (assert (and (= (dumped "off") (sort files))
                   (= (dumped image) (sort files)))
              (string "each named file dumps once with the boot image off and "
                      "once from the image, but these ran off: " (dumped "off")
                      " and these from the image: " (dumped image))))))

(eprintln "all boot-parity scenarios passed")
