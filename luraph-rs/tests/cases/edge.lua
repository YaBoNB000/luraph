-- edge.lua: empty/odd constructs
local function empty()
end
print("empty fn:", empty() == nil)
local t = {}
print("empty tab:", #t == 0)
local s = ""
print("empty str:", #s == 0, s == "")
local function retnothing()
	return
end
print("ret nothing:", retnothing() == nil)
local a = a -- global self-read (a is nil global)
print("global selfread:", a == nil)
local b = 5
local c = b
print("copy:", b, c)
-- unary chains
print("unary:", - - 5, not not true, not (not true), # { 1, 2 })
-- deeply nested parens
print("parens:", (((1 + 2)) * ((3 - 1))))
-- and/or short-circuit (no side effect on right)
local called = 0
local function side()
	called = called + 1
	return 1
end
local r1 = true or side()
local r2 = false and side()
print("shortcircuit:", r1 == true, r2 == false, called == 0)
-- function call as statement
print("callstmt:", 1)
-- semicolons
local d = 1;
local e = 2;
print("semi:", d + e)
-- if with complex condition
local f = 15
if f > 10 and f < 20 or f == 99 then
	print("complex cond: ok")
end
-- while true with break via computed value
local steps = { 3, 5, 7 }
local idx = 0
local acc = 0
while true do
	idx = idx + 1
	if idx > #steps then
		break
	end
	acc = acc + steps[idx]
end
print("while steps:", acc)
-- repeat with complex until
local m = 1
repeat
	m = m * 2
until (m > 8) and (m < 32)
print("repeat complex:", m)
-- nested function with upvalue mutation
local function make()
	local data = 0
	local function set(v)
		data = v
	end
	local function get()
		return data
	end
	set(99)
	return get()
end
print("maker:", make())
-- table with function values
local reg = {}
reg.add = function(a, b)
	return a + b
end
reg.sub = function(a, b)
	return a - b
end
print("regtable:", reg.add(10, 5), reg.sub(10, 5))
-- VM tail-invariant edges: break as the function's LAST statement (loop
-- end label must land on the implicit trailing return, never past EOB)
local function tailbreak(limit)
	local n = 0
	while true do
		n = n + 1
		if n >= limit then
			break
		end
	end
	return n
end
print("tailbreak:", tailbreak(5), tailbreak(1))
-- all-branches-return if as the function's last statement (label past
-- the final return is only ever targeted by dead jumps)
local function tailret(x)
	if x > 0 then
		return "pos"
	elseif x < 0 then
		return "neg"
	else
		return "zero"
	end
end
print("tailret:", tailret(3), tailret(-2), tailret(0))
print("edge done")
