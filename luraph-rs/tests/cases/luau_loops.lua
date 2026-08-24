-- luau_loops.lua: continue edge semantics (Luau-only)
-- continue in for-numeric: increment runs, body skipped
local c1 = 0
for n = 1, 12 do
	if n % 3 == 0 then
		continue
	end
	c1 = c1 + n
end
print("contnum:", c1)
-- continue in for-numeric: loop variable advances
local seen = {}
for n = 1, 6 do
	if n % 2 == 0 then
		continue
	end
	seen[#seen + 1] = n
end
print("contcap:", seen[1], seen[2], seen[3])
-- continue in for-in
local c3 = 0
for k, v in pairs({ a = 1, b = 2, c = 3 }) do
	if k == "b" then
		continue
	end
	c3 = c3 + v
end
print("contgen:", c3)
-- continue in repeat
local r = 0
local s = 0
repeat
	r = r + 1
	if r % 2 == 0 then
		continue
	end
	s = s + r
until r >= 6
print("contrepeat:", s)
-- break + continue combined in nested for
local t = 0
for i = 1, 4 do
	for j = 1, 4 do
		if j == 1 then
			continue
		end
		if i == 2 and j == 3 then
			break
		end
		t = t + i * 10 + j
	end
end
print("nestedmix:", t)
print("luau loops done")
