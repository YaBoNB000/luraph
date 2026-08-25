-- Multi-value protocol: truncation, expansion, select, vararg forwarding
local function m3(a, b, c) return a, b, c end
local function m1(x) return x end
local function mnone() end

local function sum(...)
	local t = { ... }
	local s = 0
	for i = 1, #t do s = s + t[i] end
	return s
end
print("exp1:", sum(1, m3(10, 20, 30), 4))
print("exp2:", sum(1, 4, m3(10, 20, 30)))
print("exp3:", sum())
print("selc1:", select('#', m1(nil)))
print("selc2:", select('#', mnone()))
print("selc3:", select('#', m3(1, 2, 3)))
print("selidx:", select(2, m3(1, 2, 3)), select(3, m3(1, 2, 3)), select(4, m3(1, 2, 3)))

-- assignment: extra slots get nil
local a, b, c = m1(5)
print("asg1:", a, b, c)
local d, e = m3(1, 2, 3)
print("asg2:", d, e)

-- table constructors
local t1 = { m3(1, 2, 3), 9 }
print("tab1:", #t1, t1[1], t1[2])
local t2 = { 9, m3(1, 2, 3) }
print("tab2:", #t2, t2[1], t2[2], t2[3], t2[4])

-- multi-return through a wrapper
local function retall() return m3(7, 8, 9) end
local r1, r2, r3, r4 = retall()
print("ret1:", r1, r2, r3, r4)

-- condition uses only the first value
local c1 = (m3(0, 1, 2) and "T" or "F")
local c2 = (m3(nil, 1, 2) and "T" or "F")
local c3 = (m3(1, 2, 3) and "T" or "F")
print("cond:", c1, c2, c3)

-- while condition
local i = 0
while m1(1) do
	i = i + 1
	if i >= 3 then break end
end
print("while:", i)

-- binary operator takes first value only
print("binop:", m3(1, 2, 3) + 10)
print("catop:", m3("a", "b", "c") .. "!")
local t3 = { v = m3(1, 2, 3) }
print("idxop:", t3.v)

-- vararg forwarding chains
local function f1(...) return ... end
local function f2(...) return f1(...) end
print("fwd1:", f2(1, 2, 3))
print("fwd2:", select('#', f2(1, 2, 3)))

-- mixed fixed + vararg
local function mixed(a, ...)
	local n = select('#', ...)
	return a, n
end
print("mix1:", mixed(9, 1, 2, 3))
print("mix2:", mixed(9))

-- multi-return in a for generator
local function genpairs()
	local n = 0
	return function()
		n = n + 1
		if n > 3 then return nil end
		return n, n * 10
	end
end
for k, v in genpairs() do
	print("forin:", k, v)
end
