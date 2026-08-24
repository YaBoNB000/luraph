-- loops.lua: for/while/repeat edge semantics
-- numeric for: empty range, backward step, single iteration
local e1 = 0
for i = 5, 1 do
	e1 = e1 + 1
end
local e2 = 0
for i = 10, 1, -2 do
	e2 = e2 + i
end
local e3 = 0
for i = 0, 0 do
	e3 = e3 + i
end
print("edges:", e1, e2, e3)
-- loop variable captured by closure (fresh per iteration)
local fns = {}
for i = 1, 4 do
	fns[i] = function()
		return i
	end
end
print("cap:", fns[1](), fns[2](), fns[3](), fns[4]())
-- body local captured by closure (fresh per iteration)
local gs = {}
for i = 1, 3 do
	local q = i * 7
	gs[i] = function()
		return q
	end
end
print("capbody:", gs[1](), gs[2](), gs[3]())
-- multi-value local inside a loop body
local m1, m2 = 0, 0
for i = 1, 3 do
	local a, b = i, i * 10
	m1 = m1 + a
	m2 = m2 + b
end
print("multival:", m1, m2)
-- local before the loop: mutated inside, mutated after
local acc = 10
for i = 1, 3 do
	acc = acc + i
end
acc = acc * 2
print("acc:", acc)
-- generic for with break
local f1 = 0
for _, v in ipairs({ 1, 2, 3, 4, 5 }) do
	if v == 3 then
		break
	end
	f1 = f1 + v
end
print("forgbreak:", f1)
-- repeat until whose cond references a body local
local r2 = 0
repeat
	r2 = r2 + 2
	local ok = r2 >= 10
until ok
print("repeatlocal:", r2)
-- global with the same name as a local (shadowing across the machine)
shadowtest = 100
local function getshadow()
	return shadowtest
end
local shadowtest = 1
for i = 1, 2 do
	shadowtest = shadowtest + i
end
print("shadow:", shadowtest, getshadow())
-- closure in a later statement referencing earlier locals
local base = 50
local mult = 2
local function later()
	return base * mult
end
print("later:", later())
-- local visible to a closure in its own initializer (compiled-scope edge)
local x, fx = 7, function()
	return x
end
print("samestmt:", x, fx())
-- break in nested for
local count = 0
for a = 1, 3 do
	for b = 1, 3 do
		if b == 2 then
			break
		end
		count = count + 1
	end
end
print("nestedbreak:", count)
-- while with break, IIFE capture of the enclosing local
local ws = {}
local w = 0
while w < 4 do
	w = w + 1
	ws[w] = (function()
		return w
	end)()
end
print("whilecap:", ws[1], ws[2], ws[3], ws[4])
print("loops done")
