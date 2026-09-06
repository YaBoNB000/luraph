-- 真实场景样本：表算法（生成 + 过滤 + 聚合）
-- 带输入：第一个变长参数为数据规模
local function make_data(n)
	local t = {}
	for i = 1, n do
		t[i] = (i * 37 + 11) % 23
	end
	return t
end

local function filter_even(t)
	local out = {}
	for i = 1, #t do
		if t[i] % 2 == 0 then
			out[#out + 1] = t[i]
		end
	end
	return out
end

local function sumsq(t)
	local s = 0
	for i = 1, #t do
		s = s + t[i] * t[i]
	end
	return s
end

local function find_max(t)
	local m = t[1]
	for i = 2, #t do
		if t[i] > m then m = t[i] end
	end
	return m
end

local n = tonumber(({ ... })[1]) or 12
local data = make_data(n)
local evens = filter_even(data)
print("sumsq:", sumsq(data))
print("even count:", #evens, "even sumsq:", sumsq(evens))
print("max:", find_max(data))
