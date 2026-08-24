-- luau_continue.lua: continue in all loop types (Luau-only)
-- while
local s1 = 0
local i = 0
while i < 10 do
	i = i + 1
	if i % 2 == 0 then
		continue
	end
	s1 = s1 + i
end
print("while cont:", s1)
-- for numeric
local s2 = 0
for n = 1, 20 do
	if n % 5 == 0 then
		continue
	end
	if n > 15 then
		break
	end
	s2 = s2 + n
end
print("for cont:", s2)
-- for generic
local s3 = 0
for _, v in ipairs({ 1, 2, 3, 4, 5 }) do
	if v % 2 == 0 then
		continue
	end
	s3 = s3 + v
end
print("ipairs cont:", s3)
-- repeat
local s4 = 0
local r = 0
repeat
	r = r + 1
	if r == 3 or r == 6 then
		continue
	end
	s4 = s4 + r
until r >= 7
print("repeat cont:", s4)
-- nested loops: continue only inner
local s5 = 0
for a = 1, 2 do
	for b = 1, 4 do
		if b == 2 then
			continue
		end
		s5 = s5 + a * 10 + b
	end
end
print("nested cont:", s5)
-- continue as last statement
local s6 = 0
local k = 0
while k < 3 do
	k = k + 1
	if k == 2 then
		continue
	end
	s6 = s6 + k
end
print("tail cont:", s6)
print("luau continue done")
