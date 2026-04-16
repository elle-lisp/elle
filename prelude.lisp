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
    (let* [[form (first forms)]
           [rest-forms (rest forms)]
           [threaded (if (pair? form)
                       `(,(first form) ,val ,;(rest form))
                       `(,form ,val))]]
      `(-> ,threaded ,;rest-forms))))

## ->> thread-last: insert value as last argument
## (->> val (f a) (g b)) => (g b (f a val))
(defmacro ->> (val & forms)
  (if (empty? forms)
    val
    (let* [[form (first forms)]
           [rest-forms (rest forms)]
           [threaded (if (pair? form)
                       `(,;form ,val)
                       `(,form ,val))]]
      `(->> ,threaded ,;rest-forms))))

## as-> - thread with named binding
## (as-> val var (f var) (g var)) => binds val to var, threads through forms
(defmacro as-> (val var & forms)
  (if (empty? forms)
    val
    (if (empty? (rest forms))
      `(let ((,var ,val)) ,(first forms))
      `(let ((,var ,val))
         (as-> ,(first forms) ,var ,;(rest forms))))))

## some-> - thread-first, short-circuiting on nil
## (some-> val (f a) (g b)) => like -> but stops if any step returns nil
(defmacro some-> (val & forms)
  (if (empty? forms)
    val
    (let* [[g (gensym)]
           [form (first forms)]
           [rest-forms (rest forms)]
           [threaded (if (pair? form)
                       `(,(first form) ,g ,;(rest form))
                       `(,form ,g))]]
      `(let ((,g ,val))
         (if (nil? ,g) nil
           (some-> ,threaded ,;rest-forms))))))

## some->> - thread-last, short-circuiting on nil
## (some->> val (f a) (g b)) => like ->> but stops if any step returns nil
(defmacro some->> (val & forms)
  (if (empty? forms)
    val
    (let* [[g (gensym)]
           [form (first forms)]
           [rest-forms (rest forms)]
           [threaded (if (pair? form)
                       `(,;form ,g)
                       `(,form ,g))]]
      `(let ((,g ,val))
         (if (nil? ,g) nil
           (some->> ,threaded ,;rest-forms))))))

## when - execute body if test is truthy, return nil otherwise
(defmacro when (test & body)
  `(if ,test (begin ,;body) nil))

## unless - execute body if test is falsy, return nil otherwise
(defmacro unless (test & body)
  `(if ,test nil (begin ,;body)))

## default - supply a default value for a &named parameter
## (default x 42) assigns x to 42 only if x is nil (not provided).
## Unlike (or), this correctly preserves explicitly-passed false values.
(defmacro default (name value)
  `(when (nil? ,name) (assign ,name ,value)))

## yield - cooperative suspension
## (yield) => (emit :yield nil)
## (yield value) => (emit :yield value)
(defmacro yield (&opt v)
  `(emit :yield ,v))

## error - signal a fiber error
## (error) => (emit :error nil)
## (error value) => (emit :error value)
(defmacro error (& args)
  (if (> (length args) 1)
    (emit :error {:error :arity-error :reason :too-many-args :maximum 1 :message "expected 0 or 1 arguments"})
    `(emit :error ,(if (empty? args) nil (first args)))))

## try/catch - error handling via fibers
## Usage: (try body... (catch e handler...))
## The last form must be (catch binding handler-body...)
## Body forms run in a fiber that catches errors.
## If an error occurs, the catch handler runs with the error bound.
## If no error occurs, the body result is returned.
(defmacro try (& forms)
  (let* [[catch-clause (last forms)]
         [body-forms (butlast forms)]
         [err-binding (first (rest catch-clause))]
         [handler-body (rest (rest catch-clause))]]
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
## (port/open, port/read-line, tcp/connect, etc.). Use protect inside
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
##       (port/read-all p)))       # body: reads contents, return value
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
  (let* [[arg-types-val (syntax->datum arg-types)]
         [arg-count (length arg-types-val)]
         [params (let [[p @[]]]
                   (letrec [[gen (fn (i)
                                   (when (< i arg-count)
                                     (push p (gensym))
                                     (gen (+ i 1))))]]
                     (gen 0))
                   (apply list p))]]
    `(def ,name
       (let ((ptr__ (ffi/lookup ,lib ,cname))
             (sig__ (ffi/signature ,ret-type ,arg-types)))
          (fn ,params
             (ffi/call ptr__ sig__ ,;params))))))

## each - iterate over a sequence
## Dispatches on type-of: lists use first/rest, indexed types use get/length.
## (each x coll body...) or (each x in coll body...)
(defmacro each (var iter-or-in & forms)
  (let* [[has-in (and (not (empty? forms))
                      (not (empty? (rest forms)))
                      (= (syntax->datum iter-or-in) 'in))]
         [iter (if has-in (first forms) iter-or-in)]
         [body (if has-in (rest forms) forms)]]
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
         ((or :struct :@struct)
          (let ((pairs (pairs seq)))
            (var idx 0)
            (var len (length pairs))
            (while (< idx len)
              (let ((,var (get pairs idx))) ,;body)
              (assign idx (+ idx 1)))))
         (:fiber
          (var v (coro/resume seq))
          (while v
            (let ((,var v)) ,;body)
            (assign v (coro/resume seq))))
         (_ (error {:error :type-error :reason :not-a-sequence :message "not a sequence"}))))))

## case - equality dispatch (flat pairs)
## (case expr val1 body1 val2 body2 ... [default])
## Uses gensym to avoid double evaluation of the dispatch expression.
## Odd element count means the last element is the default.
(defmacro case (expr & clauses)
  (let* [[g (gensym)]]
    (letrec [[build (fn (cs)
                      (if (empty? cs)
                        nil
                        (if (empty? (rest cs))
                          (first cs)
                          `(if (= ,g ,(first cs))
                             ,(first (rest cs))
                             ,(build (rest (rest cs)))))))]]
      `(let ((,g ,expr))
         ,(build clauses)))))

## if-let - conditional binding
## (if-let ((x expr) ...) then else)
## Each binding is evaluated and checked for truthiness.
## If any binding value is falsy, the else branch runs.
(defmacro if-let (bindings then else)
  (if (empty? bindings)
    then
    (let* [[b (first bindings)]
           [name (first b)]
           [val (first (rest b))]]
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

## when-ok - protect + destructure in one step
## (when-ok [val (expr)] body...) => runs body with val if expr succeeds
## Returns nil when expr errors (body is skipped).
(defmacro when-ok (binding & body)
  (let* [[name (first binding)]
         [expr (first (rest binding))]]
    `(let [[[ok? val] (protect ,expr)]]
       (when ok?
         (let [[,name val]]
           ,;body)))))

## forever - infinite loop
## (forever body...)
## Expands to (while true body...)
## Use (break) or (break value) to exit.
(defmacro forever (& body)
  `(while true ,;body))

## repeat - run body N times
## (repeat 3 (println "hi")) prints "hi" three times
(defmacro repeat (n & body)
  (let* [[g-n (gensym)]
         [g-i (gensym)]]
    `(let ((,g-n ,n))
       (var ,g-i 0)
       (while (< ,g-i ,g-n)
         ,;body
         (assign ,g-i (+ ,g-i 1))))))

## apply - call function with args spread from final list argument
## (apply f args) => (f (splice args))
## (apply f a b args) => (f a b (splice args))
(defmacro apply (f & args)
  (if (empty? args)
    `(,f)
    (let* [[last-arg (last args)]
            [init-args (butlast args)]]
      (if (empty? init-args)
        `(,f (splice ,last-arg))
        `(,f ,;init-args (splice ,last-arg))))))

## ffi/with-stack - scoped FFI stack allocations
## (ffi/with-stack [[p :int 42] [buf 64]] body...)
## Each binding is [name type value] for typed scalars or [name size] for
## raw buffers. Pointers are malloc'd, written, and freed on scope exit.
(defmacro ffi/with-stack (bindings & body)
  (if (empty? bindings)
    `(begin ,;body)
    (let* [[b (first bindings)]
           [name (first b)]
           [rest-b (rest b)]
           [rest-bindings (rest bindings)]
           [inner (if (empty? rest-bindings)
                    `(begin ,;body)
                    `(ffi/with-stack ,rest-bindings ,;body))]]
      (if (= (length b) 3)
        # [name type value] — typed scalar
        (let* [[typ (first rest-b)]
               [val (first (rest rest-b))]]
          `(let ((,name (ffi/malloc (ffi/size ,typ))))
             (ffi/write ,name ,typ ,val)
             (defer (ffi/free ,name) ,inner)))
        # [name size] — raw buffer
        (let* [[size (first rest-b)]]
          `(let ((,name (ffi/malloc ,size)))
             (defer (ffi/free ,name) ,inner)))))))

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
