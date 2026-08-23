-- basics.lua: operators, precedence, literals
-- shared corpus: must run identically on lua5.1 and luau
print(1 + 2, 5 - 8, 3 * 4, 7 / 2, 7 % 3, 2 ^ 10)
print(-2 ^ 2, 2 ^ -3, -(2 ^ 2), (-2) ^ 2)
print(1 + 2 * 3, (1 + 2) * 3, 10 % 3 / 2, 10 / (3 / 2))
print(1 .. 2 .. 3, (1 .. 2) .. 3)
print(not true, not nil, not 0, not not false)
print(true and false, nil or "x", false or nil)
print(1 < 2, 2 <= 2, 3 > 4, 4 >= 5, 1 == 1, 1 ~= 2)
print(0x1F, 0X10, 1.5, .5, 5., 1e3, 2.5e-2, 0.001)
print(7 - 2, 3 * 4.0, 1 / 4, 10 % 4)
print(1000000000000 + 1, 2 ^ 52 + 1)
local a, b, c = 1, 2, nil
print(a, b, c, # { 1, 2, 3 }, # "four", # {})
print(type(1), type(1.5), type("s"), type(true), type(nil), type(print), type({}))
