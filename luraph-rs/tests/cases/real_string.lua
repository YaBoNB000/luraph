-- 真实场景样本：字符串处理（Caesar 密码 + 统计）
-- 带输入：第一个变长参数为待处理字符串，第二个为位移量
local function caesar(s, shift)
	local out = {}
	for i = 1, #s do
		local c = string.byte(s, i)
		if c >= 97 and c <= 122 then
			c = (c - 97 + shift) % 26 + 97
		elseif c >= 65 and c <= 90 then
			c = (c - 65 + shift) % 26 + 65
		end
		out[i] = string.char(c)
	end
	return table.concat(out)
end

local function count_vowels(s)
	local n = 0
	for i = 1, #s do
		local c = string.byte(s, i)
		if c == 97 or c == 101 or c == 105 or c == 111 or c == 117
			or c == 65 or c == 69 or c == 73 or c == 79 or c == 85 then
			n = n + 1
		end
	end
	return n
end

local args = { ... }
local input = args[1] or "Hello, World!"
local shift = tonumber(args[2]) or 3
print(caesar(input, shift))
print("vowels:", count_vowels(input))
