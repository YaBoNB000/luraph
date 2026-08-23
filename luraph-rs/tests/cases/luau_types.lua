-- luau_types.lua: type annotations + aliases (parsed & dropped, Luau-only)
-- NOTE: 0.735 uses classic `function ... end` bodies (brace bodies `{}` are
-- newer Luau syntax and are NOT supported by this target).
type Point = { x: number, y: number }
type Vec3 = { x: number, y: number, z: number }
type Callback = (event: string, value: number) -> boolean
type Maybe = string | number | nil
type Pair<T, U> = { a: T, b: U }
type Nested = Map<string, Pair<number, string>>
export type Shared = { id: number }

local function makePoint(x: number, y: number): Point
	local p: Point = { x = x, y = y }
	return p
end
print("types:", makePoint(3, 4).x, makePoint(3, 4).y)

local name: string = "annotated"
local count: number = 5
local flag: boolean = true
local nothing: nil = nil
print("locals:", name, count, flag, nothing == nil)

local function sum(values: { number }): number
	local total: number = 0
	for i, v in ipairs(values) do
		total += v
	end
	return total
end
print("typed fn:", sum({ 1, 2, 3, 4 }))

local mixed: string | number = "first"
mixed = 123
print("union:", mixed)

local function typedCall(cb: Callback): boolean
	return cb("ping", 7)
end
print("typed cb:", typedCall(function(event: string, value: number): boolean
	return value > 5 and event == "ping"
end))

local list: { number } = { 1, 2, 3 }
print("table type:", #list, list[1])

-- generic alias use
local pr: Pair<number, string> = { a = 1, b = "two" }
print("generic:", pr.a, pr.b)

-- intersection type annotation
local combo: number & { meta: string } = 1
print("intersection:", combo)

print("luau types done")
