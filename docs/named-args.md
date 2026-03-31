# Named Arguments

Elle supports optional positional parameters, named keyword parameters,
and collected keyword arguments.

## &opt — optional positional

Parameters after `&opt` are `nil` if not provided.

```lisp
(defn greet [name &opt greeting]
  (println (or greeting "Hello") ", " name "!"))

(greet "Alice")            # Hello, Alice!
(greet "Bob" "Hey")        # Hey, Bob!
```

## &named — named keyword parameters

Parameters after `&named` are passed by keyword at call sites.

```lisp
(defn connect [host port &named timeout]
  [host port timeout])

(connect "localhost" 8080 :timeout 30)  # => ["localhost" 8080 30]
(connect "localhost" 8080)              # => ["localhost" 8080 nil]
```

### default

Use `default` to set default values for named parameters:

```lisp
(defn open-window [&named title width height]
  (default title "Elle")
  (default width 800)
  (default height 600)
  {:title title :width width :height height})

(open-window)
# => {:title "Elle" :width 800 :height 600}

(open-window :title "Demo" :width 1024)
# => {:title "Demo" :width 1024 :height 600}
```

## &keys — keyword args collected as struct

`&keys` collects all keyword arguments into a struct.

```lisp
(defn request [method path &keys opts]
  [method path opts])

(request "GET" "/" :timeout 30 :headers {:accept "text/html"})
# => ["GET" "/" {:timeout 30 :headers {:accept "text/html"}}]
```

---

## See also

- [functions.md](functions.md) — fn, defn, closures
- [structs.md](structs.md) — keyword args are structs
