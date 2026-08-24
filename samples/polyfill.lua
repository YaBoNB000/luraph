-- /home/user/luraph/samples/polyfill.lua
-- Roblox Buffer/Vector3 polyfill for running the luraph15 sample on Luau CLI.
-- Little-endian, 1-based positions (Roblox semantics). Includes write tracking.

local B = {}
local mt = { __index = B }
B.__index = B

local _allbufs = {}
function B.create(n)
	local o = setmetatable({ _s = string.rep("\0", n), _n = n, _id = #_allbufs + 1 }, mt)
	_allbufs[#_allbufs + 1] = o
	return o
end

function B.fromstring(s)
	local o = setmetatable({ _s = s, _n = #s, _id = #_allbufs + 1 }, mt)
	_allbufs[#_allbufs + 1] = o
	return o
end

function B.len(b) return b._n end
function B.tostring(b) return b._s end

function B.fill(b, v, s, e)
	s = s or 1
	e = e or b._n
	local cs = string.rep(string.char(v % 256), e - s + 1)
	b._s = string.sub(b._s, 1, s - 1) .. cs .. string.sub(b._s, e + 1)
	return b
end

function B.copy(dest, dp, src, sp, n)
	local chunk = string.sub(src._s, sp, sp + n - 1)
	dest._s = string.sub(dest._s, 1, dp - 1) .. chunk .. string.sub(dest._s, dp + n)
	return dest
end

local R = {
	readu8 = "<B", readi8 = "<b", readu16 = "<H", readi16 = "<h",
	readu32 = "<I", readi32 = "<i", readf32 = "<f", readf64 = "<d",
}
for name, fmt in pairs(R) do
	B[name] = function(b, pos)
		return (string.unpack(fmt, b._s, pos))
	end
end

function B.readstring(b, pos)
	local s = string.sub(b._s, pos)
	local l = 0
	for i = 1, #s do
		if s:sub(i, i) == "\0" then l = i - 1 break end
		l = i
	end
	return string.sub(s, 1, l), l
end

local W = {
	writeu8 = "<B", writei8 = "<b", writeu16 = "<H", writei16 = "<h",
	writeu32 = "<I", writei32 = "<i", writef32 = "<f", writef64 = "<d",
}
for name, fmt in pairs(W) do
	B[name] = function(b, pos, v)
		local enc = string.pack(fmt, v)
		b._s = string.sub(b._s, 1, pos - 1) .. enc .. string.sub(b._s, pos + #enc)
		return b
	end
end

function B.dump_all()
	for _, b in ipairs(_allbufs) do
		print(string.format("[buffer#%d] len=%d first64hex:", b._id, b._n))
		local out = {}
		for i = 1, math.min(64, b._n) do
			out[i] = string.format("%02X", string.byte(b._s, i))
		end
		print(table.concat(out, " "))
	end
end
B.dump_all = B.dump_all

-- Vector3 stub
local V3 = {}
V3.__index = V3
function V3.new(x, y, z)
	return setmetatable({ x = x, y = y, z = z, __v3 = true }, V3)
end
function V3.__tostring(self)
	return string.format("Vector3(%g, %g, %g)", self.x, self.y, self.z)
end

buffer = B
Vector3 = V3
vector = { create = V3.new }
return B
