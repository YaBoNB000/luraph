-- multi_proto.lua: 多原型压力用例（增量㊻ / R018 报告 P2）：单程序内
-- 高原型数 + 原型间交互（互递归/闭包工厂/元表方法路由）+ 中等常量池。
-- 用途：让块钥匙/链哈希/常量池分段等防御机制有真实负载面可验证，
-- 同时给运行等价矩阵补大规模原型场景。输出必须确定性。

-- 递归（深栈）
local function fact(n)
	if n <= 1 then return 1 end
	return n * fact(n - 1)
end

local function fib(n)
	if n < 2 then return n end
	return fib(n - 1) + fib(n - 2)
end

-- 互递归（两个原型互相引用对方的上值）
local is_even, is_odd
is_even = function(n)
	if n == 0 then return true end
	return is_odd(n - 1)
end
is_odd = function(n)
	if n == 0 then return false end
	return is_even(n - 1)
end

-- 闭包工厂（每调用一次生成新原型实例，各带独立上值状态）
local function make_counter(start)
	local c = start
	return function(step)
		c = c + step
		return c
	end
end

local function make_accum()
	local total = 0
	local count = 0
	return function(v)
		total = total + v
		count = count + 1
		return total, count
	end
end

-- varargs + select
local function sum_var(...)
	local n = select("#", ...)
	local s = 0
	for i = 1, n do
		local v = select(i, ...)
		s = s + v
	end
	return s, n
end

-- 元表方法路由（__index 函数作为方法分发器）
local registry = { base = 10 }
local mt = {
	__index = function(t, k)
		if k == "double" then return t.base * 2 end
		if k == "triple" then return t.base * 3 end
		if k == "square" then return t.base * t.base end
		return nil
	end,
}
setmetatable(registry, mt)

-- 常量池负载：字符串表 + 拼接器
local words = { "alpha", "beta", "gamma", "delta", "epsilon", "zeta" }
local function join_words(sep)
	local out = ""
	for i = 1, #words do
		if i > 1 then out = out .. sep end
		out = out .. words[i]
	end
	return out
end

-- 错误路径（pcall 包 error，跨原型）
local function try_div(a, b)
	local ok, r = pcall(function()
		if b == 0 then error("div by zero") end
		return a / b
	end)
	if ok then return r end
	return "ERR"
end

-- 查表 + 混合运算
local lut = {}
for i = 1, 32 do
	lut[i] = (i * 7 + 3) % 11
end
local function lut_sum(lo, hi)
	local s = 0
	for i = lo, hi do
		s = s + lut[(i - 1) % 32 + 1]
	end
	return s
end

print("fact:", fact(6), fact(10) % 1000)
print("fib:", fib(10), fib(15))
print("parity:", is_even(10), is_odd(10), is_even(7), is_odd(7))

local c1 = make_counter(5)
local c2 = make_counter(100)
print("counters:", c1(3), c1(4), c2(7), c2(1))

local acc = make_accum()
print("accum:", acc(10), acc(20), acc(5))

local s, n = sum_var(1, 2, 3, 4, 5)
print("var:", s, n)
local s2, n2 = sum_var(9, 8, 7)
print("var2:", s2, n2)

print("meta:", registry.double, registry.triple, registry.square, registry.base)
print("words:", join_words("-"))
print("words2:", join_words("|"))
print("div:", try_div(10, 4), try_div(1, 0), try_div(9, 3))
print("lut:", lut_sum(1, 16), lut_sum(10, 40))
