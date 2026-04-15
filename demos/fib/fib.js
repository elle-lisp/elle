// Fibonacci benchmark — naive recursive, fib(30) = 832040
// Tests raw function call overhead: ~2.7M calls

function fib(n) {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}

const result = fib(30);
println("fib(30) = ", result);
