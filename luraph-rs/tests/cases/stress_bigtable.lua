-- Table stress: big builds, sort, traversal, rawget/rawset, nesting
local N = 20000
local t = {}
for i = 1, N do
	t[i] = i * i
end
local s = 0
for i = 1, N do
	s = s + t[i]
end
print("sum:", s)

local arr = {}
for i = 1, 500 do
	arr[i] = (i * 7919) % 500
end
table.sort(arr)
print("sorted:", arr[1], arr[2], arr[3], arr[500])

local cnt = 0
local k
while true do
	k = next(t, k)
	if k == nil then break end
	cnt = cnt + 1
end
print("next:", cnt)

local sp = {}
sp[1000] = "x"
sp[1] = 1
print("sparse:", sp[1000], next(sp, 1))

local r = setmetatable({}, { __index = function() return "meta" end })
rawset(r, "k", 1)
print("raw:", r.k, rawget(r, "k"), r["nope"], rawget(r, "nope"))

local d = {}
local cur = d
for i = 1, 30 do
	cur.n = i
	cur.next = {}
	cur = cur.next
end
cur = d
local last = 0
while cur do
	last = cur.n
	cur = cur.next
end
print("deep:", last)

-- hash part traversal
local h = { a = 1, bb = 2, ccc = 3 }
local keys = {}
for key, val in pairs(h) do
	keys[#keys + 1] = key
end
table.sort(keys)
print("hash:", table.concat(keys, ","), h[1])

-- mixed table (no duplicate keys: 5.1 and Luau resolve those
-- differently — 5.1 lets positional fields win, Luau is last-write)
local mixed = { 10, 20, name = "x" }
print("mixed:", mixed[1], mixed[2], mixed.name, #mixed)

-- table.remove/insert
local li = { 1, 2, 3, 4, 5 }
table.insert(li, 3, 99)
print("ins:", li[1], li[2], li[3], li[4], li[5])
table.remove(li, 2)
print("rem:", li[1], li[2], li[3], li[4], #li)
