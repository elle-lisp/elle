(elle/epoch 14)
# audited: 2026-09-30
# A macro's & parameter collects the arguments past its fixed ones, and a call short of the fixed ones fails to expand.
# docs/macros.md

(defmacro my-list (& items)
  `(list ,;items))

(defmacro my-add (first & rest)
  `(+ ,first ,;rest))

(defmacro my-when (test & body)
  `(if ,test
     (begin
       ,;body)
     nil))

(assert (= (my-list 1 2 3) (list 1 2 3))
        "a rest parameter alone collects every argument")
(assert (= (my-list) (list))
        "a rest parameter collects nothing from no arguments")
(assert (= (my-add 1 2 3) 6)
        "a fixed parameter takes its argument before the rest parameter")
(assert (= (my-when true 1 2 3) 3)
        "a rest parameter spliced into a body runs every form")

(let [[ok? _] (protect ((fn ()
                          (eval '(begin
                                   (defmacro at-least-two (a b & rest)
                                     `(list ,a ,b ,;rest))
                                   (at-least-two 1))))))]
  (assert (not ok?) "a call short of the fixed parameters fails to expand"))
