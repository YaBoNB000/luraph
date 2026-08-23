-- luau_escapes.lua: \x hex escapes (Luau-only; 5.1 treats \x as literal)
local a = "\x44\x45"
print("hex2:", #a, a)
-- Luau requires exactly two hex digits after \x (verified against CLI)
local b = "\x04"
print("hex:", #b, b == "\4")
local c = "\x45\69"
print("mixed:", c)
print("luau escapes done")
