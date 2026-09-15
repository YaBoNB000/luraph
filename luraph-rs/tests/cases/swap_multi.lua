-- swap_multi: 多赋值语义回归（㉜：RHS 必须先全量求值再统一存储）
-- 覆盖：同表索引交换、局部+索引混存、全局目标、调用展开、vararg 展开、
-- 右侧读被写目标、少值补 nil、多值丢弃。

-- 1. 同表索引交换（堆/排序核心形态）
local t = {}
for i = 1, 20 do t[i] = (i * 37) % 20 end
for pass = 1, #t do
	for i = 1, #t - 1 do
		if t[i] > t[i + 1] then
			t[i], t[i + 1] = t[i + 1], t[i]
		end
	end
end
local s = {}
for i = 1, #t do s[i] = tostring(t[i]) end
print("sort:", table.concat(s, ","))

-- 2. 变量+索引混合、右值读将被写的槽
local a, b = 10, 20
local u = { 1, 2, 3 }
a, u[2], b = u[2], b, a
print("mix:", a, u[2], b)

-- 3. 同表非相邻键 + 下标是表达式
local m = { 5, 3, 8, 1, 9 }
local i, j = 1, 5
m[i], m[j] = m[j], m[i]
m[i + 1], m[j - 1] = m[j - 1], m[i + 1]
print("m:", m[1], m[2], m[3], m[4], m[5])

-- 4. 调用展开进多目标（含索引目标）
local function two() return 111, 222 end
local p, q = 0, 0
p, q = two()
local w = { 9, 9, 9 }
w[1], w[3] = two()
print("call:", p, q, w[1], w[2], w[3])

-- 5. 少值补 nil + 值多于目标（副作用求值）
local x, y, z = 1, 2, 3
x, y, z = 7
local side = 0
local function bump() side = side + 1 return 42 end
local only = 0
only, x = bump(), bump(), bump()
print("nil/extra:", x, y, z, only, side)

-- 6. vararg 展开进索引目标
local function vpick(...)
	local v = { 0, 0, 0 }
	v[1], v[2] = ...
	return v[1], v[2], v[3]
end
print("varg:", vpick(55, 66))

-- 7. 交换后链式依赖：下一步读被交换的表
local grid = { { c = 1 }, { c = 2 }, { c = 3 } }
local gi, gj = 1, 3
grid[gi], grid[gj] = grid[gj], grid[gi]
print("grid:", grid[1].c, grid[2].c, grid[3].c, grid[gi].c + grid[gj].c)
