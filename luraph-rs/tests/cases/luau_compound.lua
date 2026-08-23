-- luau_compound.lua: compound assignments (Luau-only)
local a = 10
a += 5
print("add:", a)
local b = 10
b -= 4
print("sub:", b)
local c = 3
c *= 4
print("mul:", c)
local d = 12
d /= 2
print("div:", d)
local e = 17
e %= 5
print("mod:", e)
local f = 2
f ^= 5
print("pow:", f)
-- compound on index
local t = { x = 1, [2] = 10 }
t.x += 1
t[2] -= 3
print("index:", t.x, t[2])
-- compound with expression RHS
local g = 2
g += 3 * 4
print("expr:", g)
local h = 100
h %= 7 + 1
print("expr mod:", h)
-- multiple statements
local i = 1
while i <= 4 do
	i += 1
end
print("loop:", i)
print("luau compound done")
