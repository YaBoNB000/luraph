-- Upvalue chains, reassignment, loop-variable capture
local a = 1
local f1 = function() return a end
a = 2
local f2 = function() return a end
print("chain:", f1(), f2())

-- numeric for: fresh variable per iteration (5.1 + Luau)
local fns = {}
for i = 1, 3 do
	fns[i] = function() return i end
end
print("cap:", fns[1](), fns[2](), fns[3]())

-- while loop capture
local w = {}
local j = 0
while j < 3 do
	j = j + 1
	w[j] = function() return j end
end
print("whilecap:", w[1](), w[2](), w[3]())

-- nested closure reassigning a shared upvalue
local x = 0
local outer = function()
	x = x + 1
	return function()
		x = x + 10
		return x
	end
end
local g = outer()
print("nested:", x, g(), x, g())

-- two closures sharing one upvalue
local p = 100
local add = function(n) p = p + n; return p end
local mul = function(n) p = p * n; return p end
add(1)
mul(2)
add(1)
print("mutual:", p)

-- vararg accessed through a closure
local function vcap(...)
	local n = select('#', ...)
	return select(n, ...)
end
local vc = function() return vcap(1, 2, 3) end
print("varg:", vc())

-- upvalue used before and after a long computation
local acc = 0
local bump = function() acc = acc + 1; return acc end
for i = 1, 1000 do
	local _ = i * 3
end
print("acc:", bump(), bump(), acc)
