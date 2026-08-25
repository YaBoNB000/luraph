-- Control-flow stress: recursion, nested loops, break, repeat,
-- conditionals with multi-value first operands
local function fib(n)
	if n < 2 then return n end
	return fib(n - 1) + fib(n - 2)
end
print("fib:", fib(15))

local s = 0
for i = 1, 10 do
	for j = 1, 10 do
		for k = 1, 10 do
			if (i + j + k) % 2 == 0 then
				s = s + 1
			end
		end
	end
end
print("trip:", s)

local found
for i = 1, 100 do
	if i * i == 49 then
		found = i
		break
	end
end
print("found:", found)

local r = 0
local i = 1
repeat
	r = r + i
	i = i + 1
until i > 5
print("repeat:", r)

-- mutual recursion
local iseven
local isodd
iseven = function(n)
	if n == 0 then return true end
	return isodd(n - 1)
end
isodd = function(n)
	if n == 0 then return false end
	return iseven(n - 1)
end
print("mutrec:", iseven(10), isodd(11), iseven(11))

-- deeply nested if
local x = 3
local res = "none"
if x == 1 then
	res = "one"
elseif x == 2 then
	res = "two"
elseif x == 3 then
	if x > 0 then
		if x < 10 then
			res = "small+"
		end
	end
end
print("nest:", res)

-- while with complex condition
local n, steps = 100, 0
while n > 1 do
	if n % 2 == 0 then
		n = n / 2
	else
		n = 3 * n + 1
	end
	steps = steps + 1
end
print("collatz:", n, steps)

-- goto-free loop transforms
local total = 0
local m = 1
while m <= 10 do
	local inner = 0
	local q = m
	while q > 0 do
		inner = inner + q % 10
		q = math.floor(q / 10)
	end
	total = total + inner
	m = m + 1
end
print("digitsum:", total)
