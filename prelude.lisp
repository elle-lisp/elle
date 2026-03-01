## Elle standard prelude
##
## Loaded automatically by the Expander before user code expansion.
## These are defmacro definitions — they register macros in the
## Expander's macro table and produce no runtime code.

## defn - function definition shorthand
## (defn f (x y) body...) => (def f (fn (x y) body...))
(defmacro defn (name params & body)
  `(def ,name (fn ,params ,;body)))

## let* - sequential bindings
## (let* ((a 1) (b a)) body...) => (let ((a 1)) (let ((b a)) (begin body...)))
(defmacro let* (bindings & body)
  (if (empty? bindings)
    `(begin ,;body)
    (if (empty? (rest bindings))
      `(let (,(first bindings)) ,;body)
      `(let (,(first bindings))
         (let* ,(rest bindings) ,;body)))))

## -> thread-first: insert value as first argument
## (-> val (f a) (g b)) => (g (f val a) b)
(defmacro -> (val & forms)
  (if (empty? forms)
    val
    (let* ((form (first forms))
           (rest-forms (rest forms))
           (threaded (if (pair? form)
                       `(,(first form) ,val ,;(rest form))
                       `(,form ,val))))
      `(-> ,threaded ,;rest-forms))))

## ->> thread-last: insert value as last argument
## (->> val (f a) (g b)) => (g b (f a val))
(defmacro ->> (val & forms)
  (if (empty? forms)
    val
    (let* ((form (first forms))
           (rest-forms (rest forms))
           (threaded (if (pair? form)
                       `(,;form ,val)
                       `(,form ,val))))
      `(->> ,threaded ,;rest-forms))))

## when - execute body if test is truthy, return nil otherwise
(defmacro when (test & body)
  `(if ,test (begin ,;body) nil))

## unless - execute body if test is falsy, return nil otherwise
(defmacro unless (test & body)
  `(if ,test nil (begin ,;body)))

## try/catch - error handling via fibers
## Usage: (try body... (catch e handler...))
## The last form must be (catch binding handler-body...)
## Body forms run in a fiber that catches errors.
## If an error occurs, the catch handler runs with the error bound.
## If no error occurs, the body result is returned.
(defmacro try (& forms)
  (let* ((catch-clause (last forms))
         (body-forms (butlast forms))
         (err-binding (first (rest catch-clause)))
         (handler-body (rest (rest catch-clause))))
    `(let ((f (fiber/new (fn () ,;body-forms) 1)))
       (fiber/resume f nil)
       (if (= (fiber/status f) :dead)
         (fiber/value f)
         (let ((,err-binding (fiber/value f)))
           ,;handler-body)))))

## protect - run body, return [success? value]
## Does not propagate errors — captures them as data.
## :dead means normal completion# anything else means error.
(defmacro protect (& body)
  `(let ((f (fiber/new (fn () ,;body) 1)))
     (fiber/resume f nil)
     [(= (fiber/status f) :dead) (fiber/value f)]))

## defer - run cleanup after body regardless of success/failure
## If the body errors, cleanup runs then the error is re-raised.
(defmacro defer (cleanup & body)
  `(let ((f (fiber/new (fn () ,;body) 1)))
     (fiber/resume f nil)
     ,cleanup
     (if (= (fiber/status f) :dead)
       (fiber/value f)
       (fiber/propagate f))))

## with - resource management (acquire/release)
## Usage: (with binding ctor dtor body...)
## Acquires the resource, runs body, then releases via destructor.
## Errors in body are propagated after cleanup.
(defmacro with (binding ctor dtor & body)
  `(let ((,binding ,ctor))
     (defer (,dtor ,binding) ,;body)))

## yield* - delegate to sub-coroutine
## Resumes the sub-coroutine, yielding each of its values to the caller.
## Resume values from the caller are passed through to the sub-coroutine.
## Returns the sub-coroutine's final value when it completes.
(defmacro yield* (co)
  `(let ((c ,co))
     (coro/resume c nil)
     (while (not (coro/done? c))
       (coro/resume c (yield (coro/value c))))
     (coro/value c)))

## ffi/defbind - convenient FFI function binding
## Usage: (ffi/defbind name lib-handle "c-name" return-type [arg-types...])
## Expands to a wrapper function that looks up the symbol, creates a signature,
## and defines a function that calls it.
## Example: (ffi/defbind abs libc "abs" :int [:int])
##   => (def abs (let ((ptr__ (ffi/lookup libc "abs"))
##                     (sig__ (ffi/signature :int [:int])))
##                 (fn (a0) (ffi/call ptr__ sig__ a0))))
(defmacro ffi/defbind (name lib cname ret-type arg-types)
  (let* ((ptr-sym (gensym))
         (sig-sym (gensym))
         (arg-types-val (syntax->datum arg-types))
         (arg-count (length arg-types-val))
         (params (letrec ((gen-params (fn (i acc)
                                        (if (= i arg-count)
                                          (reverse acc)
                                          (gen-params (+ i 1) (cons (gensym) acc))))))
                   (gen-params 0 '())))
         (call-args params))
    `(def ,name
       (let ((,ptr-sym (ffi/lookup ,lib ,cname))
             (,sig-sym (ffi/signature ,ret-type ,arg-types)))
          (fn ,params
             (ffi/call ,ptr-sym ,sig-sym ,;call-args))))))

## each - iterate over a sequence
## Dispatches on type: lists use first/rest, indexed types use get/length,
## strings use char-at/length.
## (each x coll body...) or (each x in coll body...)
(defmacro each (var iter-or-in & forms)
  (let* ((has-in (and (not (empty? forms))
                      (not (empty? (rest forms)))
                      (= (syntax->datum iter-or-in) 'in)))
         (iter (if has-in (first forms) iter-or-in))
         (body (if has-in (rest forms) forms))
         (g-iter (gensym))
         (g-idx (gensym))
         (g-len (gensym))
         (g-cur (gensym)))
    `(let ((,g-iter ,iter))
       (cond
         ((empty? ,g-iter) nil)
         ((pair? ,g-iter)
          (let* ((,g-cur ,g-iter))
            (while (pair? ,g-cur)
              (begin
                (let ((,var (first ,g-cur)))
                  ,;body)
                (set ,g-cur (rest ,g-cur))))))
         ((or (array? ,g-iter) (tuple? ,g-iter) (bytes? ,g-iter) (blob? ,g-iter))
          (let* ((,g-len (length ,g-iter))
                 (,g-idx 0))
            (while (< ,g-idx ,g-len)
              (begin
                (let ((,var (get ,g-iter ,g-idx)))
                  ,;body)
                (set ,g-idx (+ ,g-idx 1))))))
         ((or (string? ,g-iter) (buffer? ,g-iter))
          (let* ((,g-len (length ,g-iter))
                 (,g-idx 0))
            (while (< ,g-idx ,g-len)
              (begin
                (let ((,var (string/char-at ,g-iter ,g-idx)))
                  ,;body)
                (set ,g-idx (+ ,g-idx 1))))))
         (true (error :type-error "each: not a sequence"))))))

## case - equality dispatch (flat pairs)
## (case expr val1 body1 val2 body2 ... [default])
## Uses gensym to avoid double evaluation of the dispatch expression.
## Odd element count means the last element is the default.
(defmacro case (expr & clauses)
  (let* ((g (gensym)))
    (letrec ((build (fn (cs)
                      (if (empty? cs)
                        nil
                        (if (empty? (rest cs))
                          (first cs)
                          `(if (= ,g ,(first cs))
                             ,(first (rest cs))
                             ,(build (rest (rest cs)))))))))
      `(let ((,g ,expr))
         ,(build clauses)))))

## if-let - conditional binding
## (if-let ((x expr) ...) then else)
## Each binding is evaluated and checked for truthiness.
## If any binding value is falsy, the else branch runs.
(defmacro if-let (bindings then else)
  (if (empty? bindings)
    then
    (let* ((b (first bindings))
           (name (first b))
           (val (first (rest b))))
      `(let ((,name ,val))
         (if ,name
           ,(if (empty? (rest bindings))
              then
              `(if-let ,(rest bindings) ,then ,else))
           ,else)))))

## when-let - conditional binding without else
## (when-let ((x expr) ...) body...)
## Sugar for (if-let bindings (begin body...) nil)
(defmacro when-let (bindings & body)
  `(if-let ,bindings (begin ,;body) nil))

## forever - infinite loop
## (forever body...)
## Expands to (while true body...)
## Use (break) or (break value) to exit.
(defmacro forever (& body)
  `(while true ,;body))

