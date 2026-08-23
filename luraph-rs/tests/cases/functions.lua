-- functions.lua: closures, upvalues, recursion, varargs, tail calls
-- basic + anonymous
local f = function(a, b)
	return a + b
end
print("anon:", f(2, 40))
-- upvalue read/write
local function counter()
	local n = 0
	return function(step)
		n = n + (step or 1)
		return n
	end
end
local c = counter()
print("upval:", c(), c(10), c(1))
-- closure capturing loop variable (each iteration fresh)
local fns = {}
for i = 1, 3 do
	fns[i] = function()
		return i
	end
end
print("loop capture:", fns[1](), fns[2](), fns[3]())
-- recursion + mutual recursion
local function fact(n)
	if n <= 1 then
		return 1
	end
	return n * fact(n - 1)
end
local even, odd
even = function(n)
	if n == 0 then
		return true
	end
	return odd(n - 1)
end
odd = function(n)
	if n == 0 then
		return false
	end
	return even(n - 1)
end
print("rec:", fact(6), even(10), odd(10), even(11))
-- tail call (deep) — both 5.1 and Luau do proper tail calls
local function tail(n)
	if n == 0 then
		return "tail-done"
	end
	return tail(n - 1)
end
-- 5000: safe on both interpreters (Luau CLI build has no TCO; Roblox
-- runtime would allow far more)
print(tail(5000))
-- varargs
local function varg(a, ...)
	local rest = { ... }
	return a + #rest, select("#", ...)
end
print("vararg:", varg(10, 1, 2, 3), varg(1))
print("vararg all:", unpack( { 5, 6, 7 } ))
-- function as value
local ops = { add = function(a, b) return a + b end, sub = function(a, b) return a - b end }
print("fnvalue:", ops.add(3, 4), ops.sub(9, 4))
-- local function: NOT visible inside its own body
local function selfref()
	return selfref ~= nil
end
print("selfref:", selfref())
-- method definition + call
local vec = {}
vec.x = 1
vec.y = 2
function vec:mag()
	return self.x + self.y
end
vec.__add = function(a, b)
	return a + b
end
print("method:", vec:mag())
-- nested functions
local function outer(a)
	local b = a * 2
	local function inner(c)
		return b + c
	end
	return inner(10)
end
print("nested:", outer(5))
