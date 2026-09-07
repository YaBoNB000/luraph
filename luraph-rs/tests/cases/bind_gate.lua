-- 增量⑱ 激活门控靶标（选项B路线一 — 输入绑定）
-- 用 --bind-key <激活值> 混淆时：包装层吃掉第一个变长参数作为激活值，
-- 其余参数转发给程序本体；激活值错误/缺失 -> 引导层直接崩溃，
-- 任何字节码都不会被解密。程序本体是真实逻辑（对输入做移位变换）。
-- 语料矩阵走无参默认路径（输出与激活值无关，可与 raw 对比）。
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
