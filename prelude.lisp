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

## error - signal a fiber error
## (error) => (emit 1 nil)
## (error value) => (emit 1 value)
(defmacro error (& args)
  (if (> (length args) 1)
    (emit 1 {:error :arity-error :message "error: expected 0 or 1 arguments"})
    `(emit 1 ,(if (empty? args) nil (first args)))))

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
## :dead means normal completion; anything else means error.
##
## WARNING: protect is synchronous. The body must not perform async I/O
## (port/open, stream/read-line, tcp/connect, etc.). Use protect inside
## ev/spawn if you need error capture around async work.
(defmacro protect (& body)
  `(let ((f (fiber/new (fn () ,;body) 1)))
     (fiber/resume f nil)
     [(= (fiber/status f) :dead) (fiber/value f)]))

## defer - run cleanup unconditionally after body, even on error.
##
## First argument:  cleanup — evaluated after body completes (success or error).
## Remaining args:  body    — evaluated in a fiber; produces the return value.
##
## Returns the body's value on success; propagates the body's error after cleanup.
##
## Example:
##   (let ((p (port/open "data.txt" :read)))
##     (defer (port/close p)         # cleanup: always runs, closes port
##       (stream/read-all p)))       # body: reads contents, return value
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
## Dispatches on type-of: lists use first/rest, indexed types use get/length.
## (each x coll body...) or (each x in coll body...)
(defmacro each (var iter-or-in & forms)
  (let* ((has-in (and (not (empty? forms))
                      (not (empty? (rest forms)))
                      (= (syntax->datum iter-or-in) 'in)))
         (iter (if has-in (first forms) iter-or-in))
         (body (if has-in (rest forms) forms)))
    `(let ((seq ,iter))
       (match (type-of seq)
         (:list
          (unless (empty? seq)
            (var cur seq)
            (while (pair? cur)
              (let ((,var (first cur))) ,;body)
              (assign cur (rest cur)))))
         ((or :array :@array :string :@string :bytes :@bytes)
          (var idx 0)
          (var len (length seq))
          (while (< idx len)
            (let ((,var (get seq idx))) ,;body)
            (assign idx (+ idx 1))))
         ((or :set :@set)
          (let ((items (set->array seq)))
            (var idx 0)
            (var len (length items))
            (while (< idx len)
              (let ((,var (get items idx))) ,;body)
              (assign idx (+ idx 1)))))
         (_ (error {:error :type-error :message "each: not a sequence"}))))))

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

## apply - call function with args spread from final list argument
## (apply f args) => (f (splice args))
## (apply f a b args) => (f a b (splice args))
(defmacro apply (f & args)
  (if (empty? args)
    `(,f)
    (let* ((last-arg (last args))
            (init-args (butlast args)))
      (if (empty? init-args)
        `(,f (splice ,last-arg))
        `(,f ,;init-args (splice ,last-arg))))))

## with-allocator - route heap allocations through a custom allocator
## (with-allocator alloc body...) => installs alloc, runs body in defer, uninstalls
## Values allocated within the body use the provided allocator.
## When the form exits, all custom-allocated objects are freed.
## Do not retain references to these objects beyond the form's dynamic extent.
(defmacro with-allocator (allocator & body)
  `(begin
     (allocator/install ,allocator)
     (defer (allocator/uninstall)
       ,;body)))
