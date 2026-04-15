#!/usr/bin/env elle
# Python Surface Syntax for Elle
#
# This file demonstrates the Python reader.
# It parses into the same Syntax trees as s-expressions;
# everything after the reader is unchanged.

# ============================================================================
# Literals
# ============================================================================

println(42)              # integer
println(3.14)            # float
println(0xFF)            # hex literal (255)
println(0b1010)          # binary literal (10)
println(0o17)            # octal literal (15)
println("hello")         # double-quoted string
println('world')         # single-quoted string
println(True)
println(False)
println(None)

# ============================================================================
# Arithmetic and operators
# ============================================================================

println(1 + 2)           # 3
println(10 - 3)          # 7
println(4 * 5)           # 20
println(10 / 2)          # 5
println(10 % 3)          # 1
println(2 ** 10)         # 1024

# Comparisons
println(1 < 2)           # true
println(1 >= 2)          # false
println(1 == 1)          # true
println(1 != 2)          # true

# Boolean operators
println(True and False)  # false
println(True or False)   # true
println(not False)       # true

# Unary minus
println(-5 + 8)          # 3

# Precedence: 1 + 2 * 3 = 7
println(1 + 2 * 3)

# ============================================================================
# Variables and assignment
# ============================================================================

x = 10
x += 1
println(x)               # 11

# ============================================================================
# Functions
# ============================================================================

# Named function
def add(a, b):
  return a + b

println(add(3, 4))       # 7

# Lambda
double = lambda n: n * 2
println(double(21))      # 42

square = lambda x: x * x
println(square(9))       # 81

# Higher-order: function returning function
def make_adder(n):
  return lambda x: n + x

add10 = make_adder(10)
println(add10(5))        # 15

# Rest parameters
def first_item(head, *tail):
  return head

println(first_item("a", "b", "c"))  # a

# Spread in function calls
def sum3(a, b, c):
  return a + b + c

args = [1, 2, 3]
println(sum3(*args))     # 6

# ============================================================================
# Control flow
# ============================================================================

# If / elif / else
grade = 85
if grade >= 90:
  println("A")
elif grade >= 80:
  println("B")           # B
else:
  println("C")

# Python ternary
println("A" if grade >= 90 else "not A")  # not A

# While loop
i = 0
while i < 3:
  i += 1

println(i)               # 3

# For loop (iterate collection)
for v in [10, 20, 30]:
  println(v)

# Break
found = None
for i in range(1, 101):
  if i * i > 50:
    found = i
    break

println(found)           # 8

# ============================================================================
# Lists and dicts
# ============================================================================

# List literal (mutable)
arr = [10, 20, 30]
println(length(arr))     # 3
println(arr[0])          # 10

# Dict literal (mutable)
point = {"x": 1, "y": 2}
println(point.x)         # 1
println(point.y)         # 2

# Field assignment
point.x = 99
println(point.x)         # 99

# Index assignment
arr[0] = 42
println(arr[0])          # 42

# List push
push(arr, 40)
println(length(arr))     # 4

# ============================================================================
# String features
# ============================================================================

name = "world"
println(f"hello {name}")               # hello world
println(f"{1 + 2} is three")           # 3 is three

# Implicit string concatenation
s = "hello" " " "world"
println(s)               # hello world

# Triple-quoted strings
multi = """line one
line two"""
println(multi)

# ============================================================================
# Higher-order functions with Elle builtins
# ============================================================================

nums = [1, 2, 3, 4, 5]
println(map(lambda x: x * x, nums))           # [1, 4, 9, 16, 25]
println(filter(lambda x: x % 2 == 0, nums))   # [2, 4]
println(fold(lambda acc, x: acc + x, 0, nums)) # 15

# ============================================================================
# Recursive functions
# ============================================================================

def fib(n):
  return n if n < 2 else fib(n - 1) + fib(n - 2)

println(fib(10))         # 55

def factorial(n):
  return 1 if n <= 1 else n * factorial(n - 1)

println(factorial(10))   # 3628800

println("done!")
