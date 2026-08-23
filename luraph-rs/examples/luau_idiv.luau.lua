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
print(math.floor(7 / 2), math.floor(7 / - 2), math.floor(- 7 / 2), math.floor(- 7 / - 2))
print(math.floor(6.72 / 2), math.floor(- 6.72 / 2))
print(math.floor(10 / 5), math.floor(4 / 4))
print(1 + math.floor(7 / 2), math.floor(1 + 7 / 2), math.floor(2 * 7 / 2), math.floor(10 % 7 / 2))
local evmrnhi5oM7l30t = 7
local D_ = 2
print(evmrnhi5oM7l30t / D_, math.floor(evmrnhi5oM7l30t / D_))
local Kliw7ygA33 = 20
Kliw7ygA33 = math.floor(Kliw7ygA33 / 3)
print(M_("\15\132\181Ǘ", "ER\143\172\209", "\12\18\14\179"), Kliw7ygA33)
print(M_("\18\149\173Ɨ", "KG\139\178\130", "\251\19\24\222"))
