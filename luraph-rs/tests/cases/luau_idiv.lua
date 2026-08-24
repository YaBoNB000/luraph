-- luau_idiv.lua: floor division // (Luau-only)
print(7 // 2, 7 // -2, -7 // 2, -7 // -2)
print(6.72 // 2, -6.72 // 2)
print(10 // 5, 4 // 4)
-- precedence: // sits at the * / % level
print(1 + 7 // 2, (1 + 7) // 2, 2 * 7 // 2, 10 % 7 // 2)
-- mixed with normal division
local a = 7
local b = 2
print(a / b, a // b)
-- compound // (desugared by our parser to a = a // b)
local c = 20
c //= 3
print("idiv compound:", c)
print("luau idiv done")
