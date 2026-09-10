-- token_mix.lua: 交错混合 + 加权校验和 + 26 进制编码（真实逻辑靶标）
-- 与 bind_gate（凯撒+滚动哈希）不同构，供 R009+ 轮次避免源程序复用污染
local function interleave(s, t)
	local out = {}
	local n, m = #s, #t
	local k = 1
	for i = 1, math.max(n, m) do
		if i <= n then
			out[k] = string.sub(s, i, i)
			k = k + 1
		end
		if i <= m then
			out[k] = string.sub(t, i, i)
			k = k + 1
		end
	end
	return table.concat(out)
end

local function checksum(s)
	local h = 0
	for i = 1, #s do
		h = (h * 37 + string.byte(s, i) * i) % 1000000007
	end
	return h
end

local function base26(x)
	if x == 0 then
		return "A"
	end
	local d = {}
	while x > 0 do
		local r = x % 26
		d[#d + 1] = string.char(65 + r)
		x = (x - r) / 26
	end
	local out = {}
	for i = #d, 1, -1 do
		out[#out + 1] = d[i]
	end
	return table.concat(out)
end

local args = { ... }
local data = {}
for i = 1, #args do
	data[i] = tostring(args[i])
end
if #data == 0 then
	data = { "mixer", "token", "7x" }
end

local parts = {}
local acc = 0
for i = 1, #data do
	local mixed = interleave(data[i], string.reverse(data[i]))
	parts[i] = mixed
	acc = (acc + checksum(mixed) * i) % 1000000007
end
print(table.concat(parts, "+"))
print("acc:", acc)
print("code:", base26(acc % 1000000))
