-- game_loop.lua: Roblox-style patterns (pure logic, no Roblox API)
-- module pattern
local M = {}
M.state = { hp = 100, level = 1, coins = 0 }

local function damage(amount)
	M.state.hp = M.state.hp - amount
	if M.state.hp < 0 then
		M.state.hp = 0
	end
end
M.damage = damage

-- event connection pattern (closures + Disconnect)
local connections = {}
local function on(event, fn)
	local id = #connections + 1
	connections[id] = { event = event, fn = fn, connected = true }
	local handle = {}
	function handle:Disconnect()
		connections[id].connected = false
	end
	return handle
end
local function fire(event, ...)
	local count = 0
	for _, c in ipairs(connections) do
		if c.event == event and c.connected then
			c.fn(...)
			count = count + 1
		end
	end
	return count
end

local hits = 0
local c1 = on("hit", function(amount)
	hits = hits + 1
	damage(amount)
end)
local c2 = on("hit", function()
	M.state.coins = M.state.coins + 1
end)
fire("hit", 10)
fire("hit", 15)
c2:Disconnect()
fire("hit", 5)
print("events:", hits, M.state.coins, M.state.hp)

-- simple "timer" loop (deterministic, no wall clock)
local now = 0
local function wait(dt)
	now = now + dt
end
local frames = 0
local function loop(until_frames)
	while frames < until_frames do
		frames = frames + 1
		wait(0.1)
	end
end
loop(50)
print("loop:", frames, now)

-- state machine (typical game FSM)
local fsm = {
	state = "idle",
	on_hit = function(s)
		s.state = "hit"
		return "hit"
	end,
	on_hit_done = function(s)
		s.state = "idle"
		return "idle"
	end,
}
print("fsm:", fsm.on_hit(fsm), fsm.on_hit_done(fsm))

-- require-style module return
return M
