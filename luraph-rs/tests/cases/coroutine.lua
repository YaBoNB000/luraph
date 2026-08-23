-- coroutine.lua: coroutines, yield, multi-value crossing
local co = coroutine.create(function(a, b)
	return a + b
end)
print("status:", coroutine.status(co))
local ok, r = coroutine.resume(co, 20, 22)
print("resume:", ok, r)
print("status after:", coroutine.status(co))
-- yield / resume
local co2 = coroutine.create(function()
	local x = coroutine.yield(1)
	return x * 2
end)
local _, v1 = coroutine.resume(co2)
local _, v2 = coroutine.resume(co2, 21)
print("yield:", v1, v2, coroutine.status(co2))
-- multi-value yield
local co3 = coroutine.create(function()
	coroutine.yield("a", "b", "c")
	coroutine.yield("d")
end)
local _, m1, m2, m3 = coroutine.resume(co3)
local _, m4 = coroutine.resume(co3)
print("multi yield:", m1, m2, m3, m4)
-- multi-value resume into yield
local co4 = coroutine.create(function()
	local p, q = coroutine.yield()
	return p + q
end)
coroutine.resume(co4)
local _, r4 = coroutine.resume(co4, 3, 4)
print("multi resume:", r4)
-- wait
local co5 = coroutine.create(function()
	local t = coroutine.wrap(function()
		coroutine.yield("wrapped")
	end)
	local w = t()
	return "got:" .. w
end)
local _, r5 = coroutine.resume(co5)
print("wrap:", r5, coroutine.status(co5))
-- status of a dead coroutine
print("dead:", coroutine.status(co2))
-- NOTE: coroutine.close is 5.2+/Luau only — tested in luau-only corpus if added
-- NOTE: coroutine.isyieldable is 5.2+/Luau only — covered in luau-only corpus
print("coroutine done")
