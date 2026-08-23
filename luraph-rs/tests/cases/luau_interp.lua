-- luau_interp.lua: backtick string interpolation (Luau-only)
local name = "world"
local n = 41
print(`hello {name}`)
print(`value: {n + 1}`)
print(`two {n} and {n * 2} end`)
local t = { x = 10 }
print(`table {t.x}`)
-- literal brace via \{
print(`brace \{ test`)
-- no placeholder at all
local plain = `just a string`
print(plain)
-- nested expression with call
local function f(a, b)
	return a * b
end
print(`call {f(3, 4)}`)
-- interpolation inside a table / call
local msg = {
	a = `a={1 + 1}`,
	b = `b={string.upper("x")}`,
}
print(msg.a, msg.b)
print(`fmt {string.format("%d", 5)}`)
-- % in template must survive (desugared via string.format %% escape)
local pct = 50
print(`pct {pct} done`)
print("percent literal: " .. `100{1}x`)
print("luau interp done")
