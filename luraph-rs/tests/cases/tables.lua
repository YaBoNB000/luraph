-- tables.lua: constructors, indexing, table.*
local t1 = { 1, 2, 3 }
print("array:", t1[1], t1[3], #t1)
local t2 = { a = 1, b = 2, ["c d"] = 3 }
print("hash:", t2.a, t2.b, t2["c d"])
local t3 = { 1, x = 5, [1 + 1] = 9, [3] = { 7, 8 } }
print("mixed:", t3[1], t3.x, t3[2], t3[3][1], t3[3][2])
local t4 = {}
print("empty:", #t4, next(t4) == nil)
-- assignment + nested
local grid = {}
for r = 1, 3 do
	grid[r] = {}
	for c = 1, 3 do
		grid[r][c] = r * 10 + c
	end
end
print("grid:", grid[2][3], grid[3][1])
-- table.*
local arr = { 5, 3, 8 }
table.insert(arr, 1, 9)
print("insert:", arr[1], #arr)
local v = table.remove(arr, 2)
print("remove:", v, arr[2])
local sorted = { 3, 1, 2 }
table.sort(sorted)
print("sort:", sorted[1], sorted[2], sorted[3])
local joined = table.concat({ "a", "b", "c" }, "-")
print("concat:", joined, #table.concat({ 1, 2 }, ""))
-- method call on table
local s = "  hello  "
print("method:", s:upper():sub(4, 8))
-- iteration over array (deterministic)
local sum = 0
for i, v in ipairs({ 10, 20, 30, 40 }) do
	sum = sum + i * v
end
print("ipairs:", sum)
-- table as function arg + return
local function wrap(x)
	return { val = x, sq = x * x }
end
local w = wrap(9)
print("wraptab:", w.val, w.sq)
print("tables done")
