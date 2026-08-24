#!/usr/bin/env python3
# Generates samples/luraph15_trace.lua: the luraph15 sample copy with opcode
# dispatch probes injected at the 5 dispatch-loop fetch sites, plus an
# in-file pcall wrapper that dumps the trace on completion or error.
import re
import sys

SRC = "samples/luraph15.lua"
DST = "samples/luraph15_trace.lua"

s = open(SRC, encoding="utf-8", errors="replace").read()

preamble = """local _lph15_log = {}
local _lph15_n = 0
local _lph15_t0 = os.clock()
local function _lph15_snapshot(tag)
	local _cnt = {}
	for i = 1, #_lph15_log do
		local k = _lph15_log[i]:match("^(%d)")
		_cnt[k] = (_cnt[k] or 0) + 1
	end
	print("TRACE[" .. tag .. "] total=" .. _lph15_n .. " logged=" .. #_lph15_log .. " elapsed=" .. string.format("%.3f", os.clock() - _lph15_t0))
	for k, v in pairs(_cnt) do
		print("TRACE[" .. tag .. "] site" .. k .. "=" .. v)
	end
end
local function _lph15_dump(tag)
	_lph15_snapshot(tag)
	print("TRACE[" .. tag .. "] first 150 ops:")
	for i = 1, math.min(150, #_lph15_log) do
		print(_lph15_log[i])
	end
	print("TRACE[" .. tag .. "] last 50 ops:")
	local a = math.max(1, #_lph15_log - 49)
	for i = a, #_lph15_log do
		print(_lph15_log[i])
	end
	print("TRACE[" .. tag .. "] mid 450..1450 (steady state):")
	for i = 450, math.min(1450, #_lph15_log) do
		print(_lph15_log[i])
	end
end
local _lph15_last_clock_check = 0
local function _lph15_tr(pc, op, site)
	_lph15_n = _lph15_n + 1
	if #_lph15_log < 20000 then
		_lph15_log[#_lph15_log + 1] = site .. ":" .. tostring(pc) .. "=" .. tostring(op)
	end
	if _lph15_n == 1000 then
		_lph15_snapshot("at1000")
	end
	if _lph15_n == 5000 then
		_lph15_snapshot("at5000")
	end
	if _lph15_n == 20000 then
		_lph15_snapshot("at20000")
	end
	if _lph15_n % 2000 == 0 and os.clock() - _lph15_last_clock_check > 1 then
		_lph15_last_clock_check = os.clock()
		if os.clock() - _lph15_t0 > 25 then
			_lph15_dump("watchdog-25s")
			error("_lph15_watchdog")
		end
	end
end
"""

stubs = """-- Roblox environment stubs (CLI has no Roblox API)
local _lph15_now = 0
local _lph15_tstart = os.clock()
local function _lph15_realtime()
	return os.clock() - _lph15_tstart
end
local function _lph15_sleep(sec)
	sec = math.min(sec or 0.1, 0.03)
	local t0 = os.clock()
	while os.clock() - t0 < sec do end
end
task = {
	wait = function(t) _lph15_now = _lph15_now + (t or 0.1); _lph15_sleep(0.01); return t or 0.1 end,
	delay = function(t, f, ...) if f then f(...) end return {} end,
	spawn = function(f, ...) if f then f(...) end end,
	defer = function(f, ...) if f then f(...) end end,
	cancel = function() end,
	throttle = function(ms, f) return function(...) if f then f(...) end end end,
	tick = function() return _lph15_realtime() end,
	time = function() return _lph15_now end,
}
wait = task.wait
tick = task.tick
delay = task.delay
spawn = task.spawn
"""

# 1) insert preamble right before the `return setmetatable(` start
i = s.find("return setmetatable(")
assert i > 0
s = s[:i] + stubs + preamble + s[i:]

# 2) inject probes at the 5 dispatch fetch sites (keep the decision ifs!)
pat = re.compile(r"local (\w+)=([A-Za-z_]\w*)\[(\w+)\];(if \1(?:>=|<)\d+ then if \1(?:>=|<)\d+ then)")
hits = [m for m in pat.finditer(s)]
print("dispatch sites found:", len(hits))
assert len(hits) >= 4
# replace from the end so offsets stay valid
for idx, m in enumerate(reversed(hits)):
	site = len(hits) - idx  # 1..N in original order
	s = s[:m.start()] + f"local {m.group(1)}={m.group(2)}[{m.group(3)}];_lph15_tr({m.group(3)},{m.group(1)},{site});{m.group(4)}" + s[m.end():]

# 3) wrap the tail call: `... :FC()(...)` -> pcall inside do/end, then dump
j = s.rfind(":FC()(...)")
assert j > 0
k = s.find("return setmetatable(")
head = s[:k]
rest = s[k:]
# rest == "return setmetatable(...):FC()(...)"
assert rest.startswith("return setmetatable(")
rest2 = rest[len("return"):]  # " setmetatable(...):FC()(...)"
m2 = rest2.rfind(":FC()(...)")
core = rest2[:m2]  # " setmetatable(TAB,{})"
tail_new = (
	core
	+ "):FC()\n"
	+ "	local _ok, _e = pcall(_lph15_main)\n"
	+ "	if not _ok then print(\"MAIN_ERR: \" .. tostring(_e)) end\n"
	+ "	_lph15_dump(\"final\")\n"
	+ "end\n"
	+ "return 0\n"
)
# need to introduce `do local _lph15_main =` — rewrite: core starts with " setmetatable("
tail_new = "do local _lph15_main = " + core + ":FC()\n\tlocal _ok, _e = pcall(_lph15_main)\n\tif not _ok then print(\"MAIN_ERR: \" .. tostring(_e)) end\n\t_lph15_dump(\"final\")\nend\nreturn 0\n"
out = head + tail_new
open(DST, "w", encoding="utf-8").write(out)
print("wrote", DST, len(out), "bytes")
