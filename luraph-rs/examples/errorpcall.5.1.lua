local g2N5gfiraxa, T3kkRgp2do_k_3, Vvo = "\201ɠ\133\130\28?M", "\161#n\2223Ղ!", "\179\251ʔ\188P8D"
local Ya7U4c = g2N5gfiraxa .. T3kkRgp2do_k_3 .. Vvo
local Xx5_bhJ3gta_s9 = {}
for l73m58 = 1, # Ya7U4c do
    Xx5_bhJ3gta_s9[l73m58] = string.byte(Ya7U4c, l73m58)
end
local function Ouqnd(...) 
    local bTrvQ3y33 = {...}
    local Ru = table.concat(bTrvQ3y33)
    local DKft_uf5 = {}
    for b67u24ypnuc7p = 1, # Ru do
        local F1aY = Xx5_bhJ3gta_s9[(b67u24ypnuc7p - 1) % # Xx5_bhJ3gta_s9 + 1]
        local FjJfy_da = string.byte(Ru, b67u24ypnuc7p) - F1aY - b67u24ypnuc7p
        local wacu_9u = FjJfy_da % 256
        if wacu_9u < 0 then
            wacu_9u = wacu_9u + 256
        end
        DKft_uf5[b67u24ypnuc7p] = string.char(wacu_9u)
    end
    return table.concat(DKft_uf5)
end
local evmrnhi5oM7l30t, D_ = pcall(function()
    return 42
end)
print(Ouqnd(":.\4", "\245\243B", "\181\192\228"), evmrnhi5oM7l30t, D_)
local Kliw7ygA33, FEyu05s78yih2Id = pcall(function()
    error(Ouqnd(",:", "\18", "\246"))
end)
print(Ouqnd(":.\4\245", "\243B\171", "\199\28g"), Kliw7ygA33, FEyu05s78yih2Id ~= nil)
local a3eo59ot3escvu, wgam, K977aVgq2h15n = pcall(function()
    return 1, 2
end)
print(Ouqnd(":.\4\245", "\243B\179\202", "\22\161\226$"), a3eo59ot3escvu, wgam, K977aVgq2h15n)
local Gi = pcall(function()
    error(Ouqnd("60", "\25\238", "\243T"), 2)
end)
print(Ouqnd(":.\4\245", "\243B\178\186", " \146\229$"), Gi)
local k4
local ejNpx83n1Joiz, gjl5 = xpcall(function()
    error(Ouqnd("B;\6\234", "\243\142s", "\186\28\159"))
end, function(M_)
    k4 = M_
    return Ouqnd("2,\17", "\237\243", "\135\170")
end)
print(Ouqnd("B;\6", "\234\243", "\142\128"), ejNpx83n1Joiz, gjl5, k4 ~= nil)
local tmd7_15 = pcall(function()
    return pcall(function()
        error(Ouqnd("39", "\17\238", "\249"))
    end)
end)
print(Ouqnd("80\22", "\253\236", "\134\128"), tmd7_15)
local Llf, vr = pcall(function()

end)
print(Ouqnd(":.\4\245", "\243B\180", "\190\22g"), Llf, vr == nil)
local z9bae0zy, emmyVnm2UM = pcall(select, Ouqnd("\237"), 1, 2, 3)
print(Ouqnd(":.\4\245\243", "B\185\186\22", "\146\220^z"), z9bae0zy, emmyVnm2UM)
local YaWj731 = 0
for c2_l = 1, 3 do
    local Aoba1Ecpa01i, lrlKzqz6 = pcall(function()
        if c2_l == 2 then
            error(Ouqnd("=?", "\18", "\249"))
        end
    end)
    if not Aoba1Ecpa01i then
        break
    end
    YaWj731 = YaWj731 + 1
end
print(Ouqnd("6:\18\249", "\167\146\169\182", "\22\153\179"), YaWj731)
print(Ouqnd("/=\21\248\249", "\146\169\182\22\153", "\153N\175Q\246"))
