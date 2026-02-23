"""Fibonacci benchmark — naive recursive, fib(30) = 832040"""

import time

def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

t0 = time.monotonic()
result = fib(30)
elapsed = time.monotonic() - t0

print(f"fib(30) = {result}")
print(f"elapsed: {elapsed * 1000:.2f} ms")
