-- errorpcall.lua: error, pcall, xpcall
-- NOTE: only ok-flags compared (error message text differs between
-- interpreters — never print it)
local ok1, v1 = pcall(function()
	return 42
end)
print("pcall ok:", ok1, v1)
local ok2, err2 = pcall(function()
	error("boom")
end)
print("pcall err:", ok2, err2 ~= nil)
-- pcall preserving multi-values on success
local ok3, a, b = pcall(function()
	return 1, 2
end)
print("pcall multi:", ok3, a, b)
-- error with level
local ok4 = pcall(function()
	error("level2", 2)
end)
print("pcall level:", ok4)
-- xpcall with handler
local captured
local ok5, v5 = xpcall(function()
	error("xpcall-err")
end, function(e)
	captured = e
	return "handled"
end)
print("xpcall:", ok5, v5, captured ~= nil)
-- nested pcall
local ok6 = pcall(function()
	return pcall(function()
		error("inner")
	end)
end)
print("nested:", ok6)
-- pcall on a function that returns nothing
local ok7, r7 = pcall(function()
end)
print("pcall nil:", ok7, r7 == nil)
-- select inside pcall
local ok8, s8 = pcall(select, "#", 1, 2, 3)
print("pcall select:", ok8, s8)
-- error in a loop breaks out via pcall
local reached = 0
for i = 1, 3 do
	local ok, e = pcall(function()
		if i == 2 then
			error("stop")
		end
	end)
	if not ok then
		break
	end
	reached = reached + 1
end
print("loop pcall:", reached)
print("errorpcall done")
