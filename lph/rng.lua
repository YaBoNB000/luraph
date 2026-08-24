-- lph/rng.lua
-- Deterministic PRNG (Park-Miller / minimal standard).
-- Pure arithmetic: identical behavior on Lua 5.1, 5.3/5.4 and Luau.

local Rng = {}

local MOD = 2147483647 -- 2^31 - 1
local A = 48271

local function seed_to_state(seed)
	local s = math.floor(math.abs(seed))
	s = s % (MOD - 1)
	if s < 1 then s = 12345 end
	return s
end

function Rng.new(seed)
	if type(seed) ~= "number" or seed ~= seed or seed == 0 then
		seed = (os.time() * 7919) + 17
	end
	local state = seed_to_state(seed)
	local r = {}

	function r.next_raw() -- returns int in [1, MOD-1]
		local lo = state % 256
		local hi = (state - lo) / 256
		-- state * A mod MOD, split to stay in exact double range
		state = (A * lo + ((A * hi) % MOD)) % MOD
		if state == 0 then state = 1 end
		return state
	end

	function r.random() -- float in (0, 1]
		return r.next_raw() / MOD
	end

	function r.int(min, max) -- int in [min, max] inclusive
		if min == nil then min = 1 end
		if max == nil then return r.int(1, min) end
		if max < min then min, max = max, min end
		local span = max - min + 1
		return min + (r.next_raw() % span)
	end

	function r.pick(list)
		return list[r.int(1, #list)]
	end

	function r.flip(prob) -- true with probability prob
		return r.random() < (prob or 0.5)
	end

	function r.bytes(n) -- n random bytes as a string
		local out = {}
		for i = 1, n do
			out[i] = string.char(r.int(0, 255))
		end
		return table.concat(out)
	end

	function r.shuffle(t)
		for i = #t, 2, -1 do
			local j = r.int(1, i)
			t[i], t[j] = t[j], t[i]
		end
		return t
	end

	return r
end

return Rng
