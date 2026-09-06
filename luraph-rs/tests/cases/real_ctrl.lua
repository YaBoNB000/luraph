-- 真实场景样本：递归 + 记忆化 + 控制流
-- 带输入：第一个变长参数为目标项数
local memo = {}
local function fib(n)
	if n < 2 then return n end
	local cached = memo[n]
	if cached then return cached end
	local r = fib(n - 1) + fib(n - 2)
	memo[n] = r
	return r
end

local function is_prime(n)
	if n < 2 then return false end
	local i = 2
	while i * i <= n do
		if n % i == 0 then return false end
		i = i + 1
	end
	return true
end

local target = tonumber(({ ... })[1]) or 10
local total = 0
local primes = 0
for i = 1, target do
	total = total + fib(i)
	if is_prime(i) then primes = primes + 1 end
end
print("fib sum:", total)
print("primes up to target:", primes)
