-- N-Queens problem solver for N=12
-- Uses standard backtracking algorithm

local N = 12
local solutions = 0

-- Check if placing a queen at (row, col) is safe
local function is_safe(board, row, col)
  -- Check column
  for r = 1, row - 1 do
    if board[r] == col then
      return false
    end
  end

  -- Check upper-left diagonal
  for r = 1, row - 1 do
    if board[r] == col - (row - r) then
      return false
    end
  end

  -- Check upper-right diagonal
  for r = 1, row - 1 do
    if board[r] == col + (row - r) then
      return false
    end
  end

  return true
end

-- Backtracking solver
local function solve(board, row)
  if row > N then
    -- All queens placed successfully
    solutions = solutions + 1
    return
  end

  for col = 1, N do
    if is_safe(board, row, col) then
      board[row] = col
      solve(board, row + 1)
      board[row] = nil
    end
  end
end

-- Main
local board = {}
solve(board, 1)

print(string.format("Solutions for N=%d: %d", N, solutions))
