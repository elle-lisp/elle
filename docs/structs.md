# Structs

Structs are key-value maps with keyword keys. `{...}` is immutable;
`@{...}` is mutable.

## Literals

```lisp
{:name "Alice" :age 30}    # immutable struct
@{:name "Alice" :age 30}   # mutable @struct
```

## Access

```lisp
(def user {:name "Alice" :age 30 :role :admin})

(get user :name)           # => "Alice"
(get user :missing :nope)  # => :nope (default value)
(has? user :age)           # => true
(length user)              # => 3

# callable struct syntax
(user :name)               # => "Alice"

# accessor syntax: obj:field is sugar for (get obj :field)
user:name                  # => "Alice"
user:role                  # => :admin
```

## Immutable updates

Operations on immutable structs return new structs.

```lisp
(def user {:name "Alice" :age 30})

(put user :email "a@b.com")   # => {:name "Alice" :age 30 :email "a@b.com"}
(del user :age)                # => {:name "Alice"}
(update user :age inc)         # => {:name "Alice" :age 31}
(merge user {:age 31 :role :admin})
# => {:name "Alice" :age 31 :role :admin}
```

## Introspection

```lisp
(keys {:a 1 :b 2})         # => (:a :b)
(values {:a 1 :b 2})       # => (1 2)
(from-pairs [[:a 1] [:b 2]])  # => {:a 1 :b 2}
```

## Nested access and update

```lisp
(def config {:db {:host "localhost" :port 5432}})

(get-in config [:db :host])          # => "localhost"
(put-in config [:db :port] 3306)     # => {:db {:host "localhost" :port 3306}}
(update-in config [:db :port] inc)   # => {:db {:host "localhost" :port 5433}}
```

## Mutable @structs

`put` and `del` on `@struct` mutate in place.

```lisp
(def tbl @{:count 0})
(put tbl :count 1)         # mutates tbl
(put tbl :name "Bob")      # adds key
tbl:count                  # => 1
tbl:name                   # => "Bob"
```

## Destructuring

```lisp
(def {:name n :age a} {:name "Alice" :age 30})
n                          # => "Alice"
a                          # => 30

# & collects remaining keys
(def {:a va & rest} {:a 1 :b 2 :c 3})
rest                       # => {:b 2 :c 3}
```

---

## See also

- [arrays.md](arrays.md) — array and @array operations
- [sets.md](sets.md) — set operations
- [destructuring.md](destructuring.md) — struct destructuring patterns
- [types.md](types.md) — mutability and type predicates
