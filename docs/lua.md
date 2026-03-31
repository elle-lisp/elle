# Lua Syntax Mode

Elle supports a Lua surface syntax for `.lua` files. The Lua reader parses
into the same syntax trees as s-expressions — everything after the reader
(expander, analyzer, lowerer, VM) is unchanged.

## Basics

```text
-- Comments use double-dash
local x = 10              -- mutable binding (var)
local y = x + 5           -- arithmetic with infix operators

println(x)                -- function calls use parens
println("hello" .. " " .. "world")  -- string concatenation
```

## Differences from standard Lua

- `local` maps to Elle's `var` (mutable binding)
- `const` maps to Elle's `def` (immutable binding)
- Strings: `"..."`, `'...'`, and `[[...]]` all work
- `~=` is inequality (maps to `not=`)
- `..` is string concatenation (maps to `string`)
- `#` is the length operator (maps to `length`)
- Tables use `{}` syntax but map to Elle structs/arrays
- `and`, `or`, `not` are boolean operators
- All Elle primitives and prelude functions are available

## Control flow

```text
if x > 0 then
  println("positive")
elseif x == 0 then
  println("zero")
else
  println("negative")
end

while x > 0 do
  x = x - 1
end

for i = 1, 10 do
  println(i)
end

for _, v in ipairs(arr) do
  println(v)
end
```

## Functions

```text
local function add(a, b)
  return a + b
end

-- Closures
local function make_adder(n)
  return function(x) return x + n end
end
```

## Running

```text
elle script.lua            -- file extension triggers Lua reader
```

---

## See also

- [syntax.md](syntax.md) — standard Lisp syntax
- [modules.md](modules.md) — imports work the same way
