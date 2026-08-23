-- control.lua: all control flow forms
-- if / elseif / else
local function classify(n)
	if n < 0 then
		return "neg"
	elseif n == 0 then
		return "zero"
	else
		return "pos"
	end
end
print(classify(-1), classify(0), classify(5))
-- nested if
local x = 3
if x > 0 then
	if x > 2 then
		print("big")
	end
end
-- while
local i = 0
while i < 4 do
	i = i + 1
end
print("while:", i)
-- while with break
local j = 0
while true do
	j = j + 1
	if j == 3 then
		break
	end
end
print("break:", j)
-- repeat until
local k = 10
repeat
	k = k - 3
until k <= 0
print("repeat:", k)
-- numeric for: forward, backward, step, no step
local s1 = 0
for n = 1, 5 do
	s1 = s1 + n
end
local s2 = 0
for n = 10, 1, -2 do
	s2 = s2 + n
end
local s3 = 0
for n = 2, 20, 6 do
	s3 = s3 + n
end
print("fornum:", s1, s2, s3)
-- generic for
local t = { 10, 20, 30 }
local p1 = 0
for _, v in ipairs(t) do
	p1 = p1 + v
end
local tbl = { a = 1, b = 2 }
local p2 = 0
for key, v in pairs(tbl) do
	p2 = p2 + v
end
print("forgen:", p1, p2)
-- custom iterator (multi-value)
local function multi()
	local i = 0
	return function()
		i = i + 1
		if i > 3 then
			return
		end
		return i, i * i
	end
end
local m1 = 0
for n, sq in multi() do
	m1 = m1 + sq
end
print("custom it:", m1)
-- do block + break inside nested loops
local count = 0
do
	for a = 1, 3 do
		for b = 1, 3 do
			if b == 2 then
				break
			end
			count = count + 1
		end
	end
end
print("nested break:", count)
-- empty blocks
do
end
if false then
	print("never")
end
print("control done")
