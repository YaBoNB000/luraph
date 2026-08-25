-- Luau-only: compound assignment, integer division, string
-- interpolation, continue
local a = 10
a += 5
a -= 2
a *= 3
a //= 2
print("comp:", a)
print("idiv:", 17 // 5, -17 // 5, 7.5 // 2)
print(`inter {a + 1} end`)
local i = 0
while true do
	i += 1
	if i == 3 then
		continue
	end
	print("loop:", i)
	if i >= 6 then
		break
	end
end
local t = { 1, 2, 3, 4 }
for idx, val in t do
	t[idx] = val * 10
end
print("mutate:", t[1], t[2], t[3], t[4])
