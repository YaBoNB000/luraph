-- Errors: pcall/xpcall result shapes, error kinds (message bodies
-- differ across dialects, so only types and counts are printed)
local ok, err = pcall(function()
	error("boom")
end)
print("e1:", ok, type(err))
local ok2, err2 = pcall(function()
	local t = {}
	return t.nope.method
end)
print("e2:", ok2, type(err2))
local ok3, r1, r2, r3 = pcall(function() return 1, 2, 3 end)
print("e3:", ok3, r1, r2, r3)
local ok4 = pcall(function() return 1, 2, 3 end)
print("e4:", select('#', ok4))
local ok5, err5 = pcall(nil)
print("e5:", ok5, type(err5))
local ok6 = pcall(function()
	return pcall(function() error("inner") end)
end)
print("e6:", ok6)
local handler = function(e) return "handled:" .. type(e) end
local ok7, v7 = xpcall(function() error({ code = 1 }) end, handler)
print("e7:", ok7, v7)
local ok8, e8 = pcall(function() error("lv0", 0) end)
print("e8:", ok8, type(e8))
local ok9, e9 = pcall(function() return (function() end) + 1 end)
print("e9:", ok9, type(e9))
local ok10, e10 = pcall(function() return (5).nope end)
print("e10:", ok10, type(e10))
local ok11, e11 = pcall(function() return "a" .. {} end)
print("e11:", ok11, type(e11))
-- error that returns a table value
local ok12, e12 = pcall(function() error({ a = 1 }) end)
print("e12:", ok12, type(e12))
-- deep pcall chain
local function deep(n)
	if n == 0 then error("bottom") end
	return deep(n - 1)
end
local ok13 = pcall(deep, 30)
print("e13:", ok13)
-- pcall around a yielding-free long computation
local ok14, v14 = pcall(function()
	local s = 0
	for i = 1, 5000 do s = s + i end
	return s
end)
print("e14:", ok14, v14)
