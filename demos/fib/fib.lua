-- Fibonacci benchmark — naive recursive, fib(30) = 832040

local function fib(n)
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end

local t0 = os.clock()
local result = fib(30)
local elapsed = os.clock() - t0

print(string.format("fib(30) = %d", result))
print(string.format("elapsed: %.2f ms", elapsed * 1000))
