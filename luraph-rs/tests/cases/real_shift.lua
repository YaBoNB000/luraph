-- 真实逻辑语料（㊴ 起；原 bind_gate.lua，激活门退役后更名）：
-- 对输入逐词凯撒位移（偏移 = 词序 + 2）+ 滚动折叠哈希。
-- 支持 `...` 输入；无参时用默认数据，输出确定、可与 raw 对比。
local args = { ... }
local data = {}
for i = 1, #args do
	data[i] = tostring(args[i])
end
if #data == 0 then
	data = { "arena", "gate", "2026" }
end

local function shift_word(w, k)
	local t = {}
	for i = 1, #w do
		local b = string.byte(w, i)
		if b >= 97 and b <= 122 then
			b = (b - 97 + k) % 26 + 97
		elseif b >= 65 and b <= 90 then
			b = (b - 65 + k) % 26 + 65
		end
		t[i] = string.char(b)
	end
	return table.concat(t)
end

local out = {}
local acc = 0
for i = 1, #data do
	local w = shift_word(data[i], i + 2)
	out[i] = w
	for j = 1, #w do
		acc = (acc * 31 + string.byte(w, j)) % 1000000007
	end
end
print(table.concat(out, "|"))
print("fold:", acc)
