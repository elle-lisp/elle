#!/usr/bin/env elle
// JavaScript Surface Syntax for Elle
//
// This file demonstrates every feature of the JS reader.
// It parses into the same Syntax trees as s-expressions;
// everything after the reader is unchanged.

// ============================================================================
// Literals
// ============================================================================

println(42);              // integer
println(3.14);            // float
println(0xFF);            // hex literal (255)
println(0b1010);          // binary literal (10)
println(0o17);            // octal literal (15)
println("hello");         // double-quoted string
println('world');         // single-quoted string
println(`template`);      // template literal (no interpolation)
println(true);
println(false);
println(null);            // → nil
println(undefined);       // → nil

// ============================================================================
// Arithmetic and operators
// ============================================================================

println(1 + 2);           // 3
println(10 - 3);          // 7
println(4 * 5);           // 20
println(10 / 2);          // 5
println(10 % 3);          // 1
println(2 ** 10);         // 1024

// String concatenation (use template literals or string())
println(`${"hello"} ${"world"}`);

// Comparisons
println(1 < 2);           // true
println(1 >= 2);          // false
println(1 === 1);         // true (strict equality)
println(1 !== 2);         // true (strict not-equal)

// Boolean operators
println(true && false);   // false
println(true || false);   // true
println(!false);          // true

// Unary minus
println(-5 + 8);          // 3

// Precedence: 1 + 2 * 3 = 7
println(1 + 2 * 3);

// ============================================================================
// Variables and assignment
// ============================================================================

// const (immutable binding)
const pi = 3.14159;
println(pi);

// let (mutable binding)
let x = 10;
x = x + 1;
println(x);               // 11

// Compound assignment
let y = 5;
y += 3;
println(y);               // 8

// Destructuring
const [first, second] = [10, 20];
println(first);            // 10
println(second);           // 20

// ============================================================================
// Functions
// ============================================================================

// Named function
function add(a, b) {
  return a + b;
}
println(add(3, 4));       // 7

// Arrow function (expression body)
const double = (n) => n * 2;
println(double(21));      // 42

// Arrow function (single param, no parens needed)
const square = n => n * n;
println(square(9));       // 81

// Arrow function (block body)
const classify = (n) => {
  if (n > 0) {
    return "positive";
  } else if (n < 0) {
    return "negative";
  } else {
    return "zero";
  }
};
println(classify(5));     // positive
println(classify(-3));    // negative
println(classify(0));     // zero

// Higher-order: function returning function
function makeAdder(n) {
  return (x) => n + x;
}
const add10 = makeAdder(10);
println(add10(5));        // 15

// Anonymous function expression
const cube = function(x) { return x * x * x; };
println(cube(3));         // 27

// Rest parameters
function firstAndRest(head, ...tail) {
  return head;
}
println(firstAndRest("a", "b", "c"));  // a

// Spread in function calls
function sum3(a, b, c) {
  return a + b + c;
}
const args = [1, 2, 3];
println(sum3(...args));    // 6

// ============================================================================
// Control flow
// ============================================================================

// If / else if / else
const grade = 85;
if (grade >= 90) {
  println("A");
} else if (grade >= 80) {
  println("B");           // B
} else {
  println("C");
}

// Ternary operator
println(grade >= 90 ? "A" : "not A");  // not A

// While loop
let i = 0;
while (i < 3) {
  i++;
}
println(i);               // 3

// For-of (iterate collection)
for (const v of [10, 20, 30]) {
  println(v);
}

// C-style for loop
let total = 0;
for (let j = 1; j <= 5; j++) {
  total += j;
}
println(total);           // 15

// For-in (iterate keys)
for (const k in {name: "Alice", age: 30}) {
  println(k);
}

// Do-while
let n = 0;
do {
  n++;
} while (n < 5);
println(n);               // 5

// Break
let found = null;
for (let i = 1; i <= 100; i++) {
  if (i * i > 50) {
    found = i;
    break;
  }
}
println(found);           // 8

// ============================================================================
// Arrays and objects
// ============================================================================

// Array literal (mutable)
let arr = [10, 20, 30];
println(length(arr));     // 3
println(arr[0]);          // 10 (0-indexed)

// Object literal (mutable)
const point = {x: 1, y: 2};
println(point.x);         // 1
println(point.y);         // 2

// Shorthand properties
const a = 1;
const b = 2;
const pair = {a, b};     // same as {a: a, b: b}
println(pair.a);          // 1

// Field assignment
point.x = 99;
println(point.x);         // 99

// Index assignment
arr[0] = 42;
println(arr[0]);          // 42

// Empty object
const obj = {};

// Nested field access
const nested = {inner: {val: 0}};
nested.inner.val = 7;
println(nested.inner.val); // 7

// Array push
push(arr, 40);
println(length(arr));     // 4

// ============================================================================
// Template literals
// ============================================================================

const name = "world";
println(`hello ${name}`);                    // hello world
println(`${1 + 2} is three`);               // 3 is three
println(`nested: ${`inner ${name}`}`);       // nested: inner world

// ============================================================================
// String escapes
// ============================================================================

println("\x48\x49");      // HI (hex escapes)
println("\t-tab-");       //     -tab-

// ============================================================================
// Error handling (try/catch)
// ============================================================================

// try/catch maps to protect
try {
  const result = 42;
  println(result);        // 42
} catch (e) {
  println("error:", e);
}

// ============================================================================
// Higher-order functions with Elle builtins
// ============================================================================

// map, filter, fold work with arrow functions
const nums = [1, 2, 3, 4, 5];
println(map((x) => x * x, nums));          // [1, 4, 9, 16, 25]
println(filter((x) => x % 2 === 0, nums)); // [2, 4]
println(fold((acc, x) => acc + x, 0, nums)); // 15

// ============================================================================
// Recursive functions
// ============================================================================

// Use ternary for expression-style recursion
function fib(n) {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}
println(fib(10));         // 55

function factorial(n) {
  return n <= 1 ? 1 : n * factorial(n - 1);
}
println(factorial(10));   // 3628800

println("done!");
