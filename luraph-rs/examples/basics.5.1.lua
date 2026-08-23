local aId, a3eo59ot3escvu, wgam = "\165\30IMr\220\220\26", "3X\140\152\157k<\220", "ҟ\135\252\153\12\249\212"
local K977aVgq2h15n = aId .. a3eo59ot3escvu .. wgam
local Gi = {}
for k4 = 1, # K977aVgq2h15n do
    Gi[k4] = string.byte(K977aVgq2h15n, k4)
end
local function M_(...) 
    local ejNpx83n1Joiz = {...}
    local gjl5 = table.concat(ejNpx83n1Joiz)
    local tmd7_15 = {}
    for Llf = 1, # gjl5 do
        local vr = Gi[(Llf - 1) % # Gi + 1]
        local z9bae0zy = string.byte(gjl5, Llf) - vr - Llf
        local emmyVnm2UM = z9bae0zy % 256
        if emmyVnm2UM < 0 then
            emmyVnm2UM = emmyVnm2UM + 256
        end
        tmd7_15[Llf] = string.char(emmyVnm2UM)
    end
    return table.concat(tmd7_15)
end
print(1 + 2, 5 - 8, 3 * 4, 7 / 2, 7 % 3, 2 ^ 10)
print(- 2 ^ 2, 2 ^ - 3, - 2 ^ 2, (- 2) ^ 2)
print(1 + 2 * 3, (1 + 2) * 3, 10 % 3 / 2, 10 / (3 / 2))
print(1 .. 2 .. 3, (1 .. 2) .. 3)
print(not true, not nil, not 0, not not false)
print(true and false, nil or M_("\30"), false or nil)
print(1 < 2, 2 <= 2, 3 > 4, 4 >= 5, 1 == 1, 1 ~= 2)
print(31, 16, 1.5, 0.5, 5.0, 1000.0, 0.025, 0.001)
print(7 - 2, 3 * 4.0, 1 / 4, 10 % 4)
print(1000000000000 + 1, 2 ^ 52 + 1)
local evmrnhi5oM7l30t, D_, Kliw7ygA33 = 1, 2, nil
print(evmrnhi5oM7l30t, D_, Kliw7ygA33, # {1, 2, 3}, # M_("\12\143", "\193", "\195"), # {})
print(type(1), type(1.5), type(M_("\25")), type(true), type(nil), type(print), type{})
