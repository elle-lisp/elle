#!/usr/bin/env elle
-- Lua Surface Syntax for Elle
--
-- This file demonstrates every feature of the Lua reader.
-- It parses into the same Syntax trees as s-expressions;
-- everything after the reader is unchanged.

-- ============================================================================
-- Literals
-- ============================================================================

println(42)              -- integer
println(3.14)            -- float
println(0xFF)            -- hex literal (255)
println("hello")         -- double-quoted string
println('world')         -- single-quoted string
println([[long
string]])                -- long string
println([==[contains ]] and still works]==])
println(true)
println(false)
println(nil)

-- ============================================================================
-- Arithmetic and operators
-- ============================================================================

println(1 + 2)           -- 3
println(10 - 3)          -- 7
println(4 * 5)           -- 20
println(10 / 2)          -- 5
println(10 % 3)          -- 1
println(math.pow(2, 10)) -- 1024

-- String concatenation
println("hello" .. " " .. "world")

-- Comparisons
println(1 < 2)           -- true
println(1 >= 2)          -- false
println(1 == 1)          -- true
println(1 ~= 2)          -- true

-- Boolean operators
println(true and false)  -- false
println(true or false)   -- true
println(not false)       -- true

-- Unary minus
println(-5 + 8)          -- 3

-- Precedence: 1 + 2 * 3 = 7
println(1 + 2 * 3)

-- ============================================================================
-- Variables and assignment
-- ============================================================================

-- Top-level local (mutable)
local x = 10
x = x + 1
println(x)               -- 11

-- Multiple assignment with destructuring (from a function returning multiple values)
function three() return 1, 2, 3 end
local a, b, c = three()
println(a)               -- 1
println(b)               -- 2
println(c)               -- 3

-- Swap via multiple assignment
local p = 100
local q = 200
p, q = q, p
println(p)               -- 200
println(q)               -- 100

-- ============================================================================
-- Functions
-- ============================================================================

-- Named function
function add(a, b)
  return a + b
end
println(add(3, 4))       -- 7

-- Local function
local function double(n)
  return n * 2
end
println(double(21))      -- 42

-- Anonymous function (closure)
local square = function(x) return x * x end
println(square(9))       -- 81

-- Higher-order: function returning function
function make_adder(n)
  return function(x) return n + x end
end
local add10 = make_adder(10)
println(add10(5))        -- 15

-- Multiple return values
function divmod(a, b)
  return a / b, a % b
end
local quot, rem = divmod(17, 5)
println(quot)            -- 3
println(rem)             -- 2

-- Varargs
function sum(...)
  return fold(function(acc, x) return acc + x end, 0, list(...))
end
println(sum(1, 2, 3, 4)) -- 10

function first_and_rest(x, ...)
  return x, list(...)
end
local head, tail = first_and_rest("a", "b", "c")
println(head)            -- a
println(tail)            -- (b c)

-- ============================================================================
-- Control flow
-- ============================================================================

-- If / elseif / else
local grade = 85
if grade >= 90 then
  println("A")
elseif grade >= 80 then
  println("B")           -- B
else
  println("C")
end

-- While loop
local i = 0
while i < 3 do
  i = i + 1
end
println(i)               -- 3

-- Numeric for
local total = 0
for j = 1, 5 do
  total = total + j
end
println(total)           -- 15

-- Numeric for with step
local evens = 0
for k = 0, 10, 2 do
  evens = evens + k
end
println(evens)           -- 30

-- For-in (generic iteration)
for x in {10, 20, 30} do
  println(x)
end

-- For-in with destructuring
for idx, val in ipairs({"a", "b", "c"}) do
  println(string(idx, ": ", val))
end

-- Repeat-until
local n = 0
repeat
  n = n + 1
until n == 5
println(n)               -- 5

-- Do block (scoping)
do
  local temp = 42
  println(temp)          -- 42
end

-- Break
local found = nil
for i = 1, 100 do
  if i * i > 50 then
    found = i
    break
  end
end
println(found)           -- 8

-- ============================================================================
-- Tables
-- ============================================================================

-- Array table
local arr = {10, 20, 30}
println(length(arr))     -- 3
println(arr[0])          -- 10 (0-indexed)

-- Struct table
local point = {x = 1, y = 2}
println(point.x)         -- 1
println(point.y)         -- 2

-- Empty table (struct)
local obj = {}

-- Field assignment
point.x = 99
println(point.x)         -- 99

-- Index assignment
arr[0] = 42
println(arr[0])          -- 42

-- Nested field assignment
local nested = {inner = {val = 0}}
nested.inner.val = 7
println(nested.inner.val) -- 7

-- Length operator
println(#arr)            -- 3

-- ============================================================================
-- String escapes
-- ============================================================================

println("\x48\x49")      -- HI (hex escapes)
println("\97\98\99")     -- abc (decimal escapes)
println("\t-tab-")       -- 	-tab-

-- ============================================================================
-- Call-without-parens sugar
-- ============================================================================

println "no parens needed"
lua_type {1, 2, 3}      -- works (returns "table")

-- ============================================================================
-- Lua standard library (auto-loaded prelude)
-- ============================================================================

-- Type checking
println(lua_type(42))        -- number
println(lua_type("hi"))      -- string
println(lua_type(true))      -- boolean
println(lua_type(nil))       -- nil
println(lua_type(println))   -- function
println(lua_type({1, 2}))    -- table

-- Conversion
println(tonumber("123"))     -- 123
println(tostring(3.14))     -- 3.14

-- Math library
println(math.sqrt(16))      -- 4
println(math.floor(3.7))    -- 3
println(math.ceil(3.2))     -- 4
println(math.abs(-5))       -- 5
println(math.pi)            -- 3.14159...
println(math.max(10, 20))   -- 20
println(math.min(10, 20))   -- 10

-- String library
println(string_lib.upper("hello"))    -- HELLO
println(string_lib.lower("WORLD"))    -- world
println(string_lib.len("test"))       -- 4
println(string_lib.trim("  hi  "))    -- hi

-- Table library
local t = {1, 2, 3}
table.insert(t, 4)
println(length(t))                    -- 4
println(table.concat(t, ", "))        -- 1, 2, 3, 4

-- Pairs / ipairs iteration
for k, v in pairs({name = "Alice", age = 30}) do
  println(string(k, " = ", v))
end

-- Error handling
local ok, result = pcall(function() return 42 end)
println(ok)              -- true
println(result)          -- 42

local ok2, err = pcall(function() lua_error("boom") end)
println(ok2)             -- false

-- Select
println(select("#", "a", "b", "c"))   -- 3
println(select(2, "a", "b", "c"))     -- b

-- Metatables (traits)
local Dog = {}
function Dog:speak()
  return "woof"
end
local d = setmetatable({name = "Rex"}, Dog)
println(d.name)                        -- Rex
println(getmetatable(d).speak)         -- shows the function

-- ============================================================================
-- Backtick escape hatch (inline s-expressions)
-- ============================================================================

-- Drop into s-expressions for any Elle feature
`(println "from s-expr land")

println "done!"
