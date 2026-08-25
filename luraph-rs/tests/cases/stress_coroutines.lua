-- Coroutines: multi-value yield, upvalues across yield, nesting
local function gen(n)
	local i = 0
	return function()
		i = i + 1
		if i > n then return nil end
		return i, i * i, "s" .. i
	end
end
local g = gen(3)
for _ = 1, 4 do
	print("gen:", g())
end

-- yield multiple values, resume with one
local co = coroutine.create(function()
	local a = coroutine.yield(10, 20, 30)
	return a * 2, "done"
end)
local ok, a, b, c = coroutine.resume(co)
print("y1:", ok, a, b, c)
local ok2, d, e = coroutine.resume(co, 5)
print("y2:", ok2, d, e, coroutine.status(co))

-- iterator-style coroutine
local items = { "a", "b", "c" }
local co2 = coroutine.create(function()
	for i, v in ipairs(items) do
		coroutine.yield(i, v)
	end
	return "finished"
end)
while true do
	local okk, i, v = coroutine.resume(co2)
	if not okk then
		print("err:", i)
		break
	end
	if i == nil then
		print("fin:", v)
		break
	end
	print("it:", i, v)
end

-- coroutine.wrap
local w = coroutine.wrap(function()
	for i = 1, 3 do
		print("wrap:", i, coroutine.yield(i * 10))
	end
	return "end"
end)
print("w1:", w(1))
print("w2:", w(2))
print("w3:", w(3))
print("w4:", w(4))

-- a coroutine that drives another coroutine
local inner = coroutine.create(function()
	coroutine.yield("i1")
	coroutine.yield("i2")
	return "i-done"
end)
local outer = coroutine.create(function()
	local _, a = coroutine.resume(inner)
	print("in:", a)
	coroutine.yield("mid")
	local _, b = coroutine.resume(inner)
	print("in:", b)
	local okc, cd = coroutine.resume(inner)
	print("in:", okc, cd)
	return "o-done"
end)
local _, r1 = coroutine.resume(outer)
print("out1:", r1)
local _, r2 = coroutine.resume(outer)
print("out2:", r2)
local _, r3 = coroutine.resume(outer)
print("out3:", r3)

-- coroutine state after resume
local co3 = coroutine.create(function() return 7 end)
print("st1:", coroutine.status(co3))
coroutine.resume(co3)
print("st2:", coroutine.status(co3))
