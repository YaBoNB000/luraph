-- multival.lua: multi-value semantics
local function multi(a, b, c)
	return a, b, c
end
local x, y, z = multi(1, 2, 3)
print("assign:", x, y, z)
local a1, a2, a3, a4 = multi(10, 20, 30)
print("extra:", a1, a2, a3, a4 == nil)
-- single target takes first value
local only = multi(7, 8, 9)
print("first:", only)
-- return multi
local function ret2()
	return 1, 2
end
local r1, r2 = ret2()
print("ret:", r1, r2)
-- # on table
local n2 = # { 1, 2, 3 }
print("#tab:", n2)
-- table constructor: trailing multi-value
local t = { multi(1, 2, 3), 99 }
print("tab tail:", t[1], t[2], t[3], t[4])
-- tail of return list
local function tailcall()
	return multi(5, 6, 7)
end
local b1, b2, b3 = tailcall()
print("tail ret:", b1, b2, b3)
-- select
print("select:", select(2, "a", "b", "c"), select("#", 1, 2, 3))
-- unpack
local u = unpack({ 4, 5, 6 })
print("unpack:", unpack( { 4, 5, 6 } ))
-- and / or with multi-value semantics
local m1 = multi(nil, "x")
local v1 = m1 or "fallback"
local v2 = (multi(1, 2)) and "yes"
print("andor:", v1, v2)
-- for-in with multi-value iterator
local seen = {}
local i = 0
local function it()
	return function()
		i = i + 1
		if i > 2 then
			return
		end
		return i, "v" .. i
	end
end
local total = 0
for n, name in it() do
	total = total + n
	table.insert(seen, name)
end
print("multi it:", total, seen[1], seen[2])
print("multival done")
