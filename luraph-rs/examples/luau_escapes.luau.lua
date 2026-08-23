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
local evmrnhi5oM7l30t = M_("\234", "e")
print(M_("\14\133", "ă", "\177"), # evmrnhi5oM7l30t, evmrnhi5oM7l30t)
local D_ = M_("\170")
print(M_("\14\133", "\196", "\139"), # D_, D_ == M_("\170"))
local Kliw7ygA33 = M_("\235", "e")
print(M_("\19\137", "Ķ", "\219\28"), Kliw7ygA33)
print(M_("\18\149\173ƗG", "V\133\157\210\252\23", "\202ݺZH"))
