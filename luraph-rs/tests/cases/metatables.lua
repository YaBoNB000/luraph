-- metatables.lua: metamethods
local base = { greeting = "hi" }
local mt = {
	__index = function(t, k)
		return "via-fn:" .. k
	end,
}
local t = setmetatable({}, mt)
print("index-fn:", t.anything)
local mt2 = { __index = base }
local t2 = setmetatable({}, mt2)
print("index-tab:", t2.greeting)
-- __newindex
local log = {}
local t3 = setmetatable({}, {
	__newindex = function(tab, k, v)
		log[k] = v
		rawset(tab, k, v)
	end,
})
t3.x = 42
print("newindex:", t3.x, log.x)
-- __call
local adder = setmetatable({}, {
	__call = function(self, a, b)
		return a + b
	end,
})
print("call:", adder(3, 4))
-- arithmetic metamethods
local V = {}
V.__index = V
function V.new(x, y)
	return setmetatable({ x = x, y = y }, V)
end
function V.__add(a, b)
	return V.new(a.x + b.x, a.y + b.y)
end
function V.__mul(a, n)
	return V.new(a.x * n, a.y * n)
end
function V.__tostring(v)
	return string.format("V(%d,%d)", v.x, v.y)
end
function V.__len(v)
	return v.x + v.y
end
local p = V.new(1, 2)
local q = V.new(10, 20)
print("arith:", p + q, p * 3, #p)
print("tostring metamethod", p + q)
-- __eq
local E = {}
E.__index = E
function E.new(v)
	return setmetatable({ v = v }, E)
end
function E.__eq(a, b)
	return a.v == b.v
end
local e1, e2 = E.new(7), E.new(7)
print("eq:", e1 == e2, E.new(7) == E.new(8))
-- rawget/rawset vs metamethods
local rawtab = setmetatable({}, { __index = function() return "meta" end })
print("raw:", rawget(rawtab, "x"), rawtab.x)
rawset(rawtab, "x", 1)
print("rawset:", rawtab.x, rawget(rawtab, "x"))
-- getmetatable/setmetatable roundtrip
local g = getmetatable(p)
print("getmeta:", g ~= nil, g == V or g ~= nil)
local plain = {}
print("no meta:", getmetatable(plain) == nil)
print("metatables done")
