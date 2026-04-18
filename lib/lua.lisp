(elle/epoch 8)
## Lua compatibility prelude
##
## Provides Lua standard library functions mapped to Elle primitives.
## Usage from .lua files:  `(import "std/lua")

# ============================================================================
# Type system
# ============================================================================

# Lua's type() returns a string; Elle's type-of returns a keyword.
(def lua_type (fn (v)
  (let [t (type-of v)]
    (if (= t :integer) "number"
      (if (= t :float) "number"
        (if (= t :string) "string"
          (if (= t :boolean) "boolean"
            (if (= t :nil) "nil"
              (if (= t :closure) "function"
                (if (= t :native-fn) "function"
                  (if (or (= t :array) (= t :@array)) "table"
                    (if (or (= t :struct) (= t :@struct)) "table"
                      "userdata"))))))))))))

# ============================================================================
# Conversion functions
# ============================================================================

(def tonumber (fn (v)
  (if (number? v) v
    (if (string? v)
      (let [[ok result] (protect ((fn () (parse-int v))))]
        (if ok result
          (let [[ok2 result2] (protect ((fn () (parse-float v))))]
            (if ok2 result2 nil))))
      nil))))

(def tostring (fn (v) (string v)))

# ============================================================================
# Output
# ============================================================================

# Lua's print adds tabs between args and a trailing newline
(def lua_print (fn (& args)
  (println (string/join (map string args) "\t"))))

# ============================================================================
# Math library — accessed as math.sqrt(), math.floor(), etc.
# ============================================================================

(def math {
  :abs    abs
  :ceil   math/ceil
  :floor  math/floor
  :sqrt   math/sqrt
  :pow    math/pow
  :sin    math/sin
  :cos    math/cos
  :tan    math/tan
  :exp    math/exp
  :log    math/log
  :pi     (math/pi)
  :huge   1.7976931348623157e308
  :maxinteger 9223372036854775807
  :mininteger -9223372036854775808
  :max    (fn (a b) (if (> a b) a b))
  :min    (fn (a b) (if (< a b) a b))
})

# ============================================================================
# String library — accessed as string_lib.upper(), etc.
# (Can't shadow `string` since that's Elle's concatenation builtin.)
# ============================================================================

(def string_lib {
  :len      length
  :find     string/find
  :upper    string/upcase
  :lower    string/downcase
  :rep      (fn (s n)
              (string/join (map (fn (_) s) (range n)) ""))
  :format   string
  :byte     (fn (s i)
              (get (bytes s) (if (nil? i) 0 (- i 1))))
  :char     (fn (n)
              (string (bytes n)))
  :trim     string/trim
})

# ============================================================================
# Table library — accessed as table.insert(), table.concat(), etc.
# ============================================================================

(def table {
  :insert  (fn (t & args)
    (if (= (length args) 1)
      (push t (get args 0))
      (push t (get args 1))))
  :remove  (fn (t & args) (pop t))
  :concat  (fn (t sep)
    (string/join (map string t) (if (nil? sep) "" sep)))
  :unpack  (fn (t) t)
})

# ============================================================================
# Iteration — pairs / ipairs
# ============================================================================

# ipairs: iterate array with 0-based index, yields [i, value] pairs
(def ipairs (fn (t)
  (def @i 0)
  (def result @[])
  (each v in t
    (push result (list i v))
    (assign i (+ i 1)))
  result))

# pairs: iterate struct keys, yields [key, value] pairs
(def pairs (fn (t)
  (def result @[])
  (each k in (keys t)
    (push result (list k (get t k))))
  result))

# ============================================================================
# Metatables → traits
# ============================================================================

# setmetatable freezes the trait table (with-traits requires immutable struct)
(def setmetatable (fn (obj mt) (with-traits obj (freeze mt))))
(def getmetatable traits)

# ============================================================================
# Error handling
# ============================================================================

# pcall: protected call — returns [true result] or [false error]
# (arrays so Lua destructuring `local ok, err = pcall(...)` works)
(def pcall (fn (f & args)
  (let [[ok result] (protect ((fn () (apply f args))))]
    (if ok
      [true result]
      [false result]))))

# lua_error: raise an error (can't use `error` — already a builtin)
(def lua_error (fn (msg)
  (if (string? msg)
    (error {:error :lua-error :message msg})
    (error msg))))

# ============================================================================
# Modules
# ============================================================================

(def require (fn (path) (import (string path ".lisp"))))

# ============================================================================
# Misc globals
# ============================================================================

(def select (fn (index & args)
  (if (= index "#")
    (length args)
    (get args (- index 1)))))

(def rawget get)
(def rawset put)
(def rawlen length)
(def rawequal =)

# Lua's assert: returns value on success, errors on nil/false
(def lua_assert (fn (v msg)
  (if v v
    (error {:error :assertion-failed
            :message (if (nil? msg) "assertion failed" msg)}))))
