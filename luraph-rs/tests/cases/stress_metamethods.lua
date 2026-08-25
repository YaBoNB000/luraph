-- Metamethod chains: __index (table/function), __newindex, __call,
-- __eq, __lt/__le (both orderings), __add, __concat, __unm
local base = { x = 1, y = 2, z = 3 }
local a = setmetatable({}, { __index = base })
print("idx1:", a.x, a.z)

local b = setmetatable({}, { __index = function(t, k)
	if k == "dyn" then return 42 end
	return base[k]
end })
print("idx2:", b.dyn, b.x)

local c = setmetatable({}, { __index = { w = 9 } })
print("idx3:", c.w, c.missing)

-- __index chain through another metatable-protected table
local root = { deep = "yes" }
local mid = setmetatable({}, { __index = root })
local leaf = setmetatable({}, { __index = mid })
print("idx4:", leaf.deep)

local nt = {}
local log = {}
setmetatable(nt, { __newindex = function(t, k, v)
	log[#log + 1] = k .. "=" .. tostring(v)
end })
nt.a = 1
nt.b = 2
nt.a = 3
print("newidx:", table.concat(log, ","))

-- __call with self
local obj = setmetatable({ k = 5 }, { __call = function(self, x, y)
	return self.k * (x + y)
end })
print("call:", obj(3, 4))

-- __eq
local EQ = function(x, y) return x.v == y.v end
local A = setmetatable({ v = 7 }, { __eq = EQ })
local B = setmetatable({ v = 7 }, { __eq = EQ })
local C = setmetatable({ v = 8 }, { __eq = EQ })
print("eq:", A == B, A == C, B == C)

-- __lt / __le in both operand orders
local P = {
	__lt = function(x, y) return x.n < y.n end,
	__le = function(x, y) return x.n <= y.n end
}
local p1 = setmetatable({ n = 1 }, P)
local p2 = setmetatable({ n = 2 }, P)
local p1b = setmetatable({ n = 1 }, P)
print("lt:", p1 < p2, p2 < p1, p1 <= p2, p2 >= p1, p1 <= p1b)

-- __add
local V3 = { __add = function(x, y)
	return { x[1] + y[1], x[2] + y[2], x[3] + y[3] }
end }
local v1 = setmetatable({ 1, 2, 3 }, V3)
local v2 = setmetatable({ 4, 5, 6 }, V3)
local v3 = v1 + v2
print("add:", v3[1], v3[2], v3[3])

-- __concat
local S = { __concat = function(x, y) return x.s .. y.s end }
local s1 = setmetatable({ s = "foo" }, S)
local s2 = setmetatable({ s = "bar" }, S)
print("concat:", s1 .. s2)

-- __unm
local U = { __unm = function(x) return -x.n end }
local u1 = setmetatable({ n = 5 }, U)
print("unm:", -u1)

-- method call through __index (self forwarding)
local M = {
	__index = {
		scale = function(self, f) return self.v * f end
	}
}
local m1 = setmetatable({ v = 6 }, M)
print("method:", m1:scale(4))
