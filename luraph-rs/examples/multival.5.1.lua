local r9Kft_u, v5_l6_u2, Ypnuc = "\133T\166%Bo+\165", "\221[dr\192?\232@", "\192\1979\213~]\221\231"
local Pvf1aY = r9Kft_u .. v5_l6_u2 .. Ypnuc
local FjJfy_da = {}
for wacu_9u = 1, # Pvf1aY do
    FjJfy_da[wacu_9u] = string.byte(Pvf1aY, wacu_9u)
end
local function ap3Fdd8(...) 
    local k97axg = {...}
    local f6vF = table.concat(k97axg)
    local DClAeh = {}
    for J4aests = 1, # f6vF do
        local dz = FjJfy_da[(J4aests - 1) % # FjJfy_da + 1]
        local efe = string.byte(f6vF, J4aests) - dz - J4aests
        local P2P9sV7l8Bztvm9 = efe % 256
        if P2P9sV7l8Bztvm9 < 0 then
            P2P9sV7l8Bztvm9 = P2P9sV7l8Bztvm9 + 256
        end
        DClAeh[J4aests] = string.char(P2P9sV7l8Bztvm9)
    end
    return table.concat(DClAeh)
end
local function evmrnhi5oM7l30t(D_, Kliw7ygA33, FEyu05s78yih2Id) 
    return D_, Kliw7ygA33, FEyu05s78yih2Id
end
local a3eo59ot3escvu, wgam, K977aVgq2h15n = evmrnhi5oM7l30t(1, 2, 3)
print(ap3Fdd8("\231\201\28", "\146\174", "\227l"), a3eo59ot3escvu, wgam, K977aVgq2h15n)
local Gi, k4, M_, ejNpx83n1Joiz = evmrnhi5oM7l30t(10, 20, 30)
print(ap3Fdd8("\235\206", "\29\155", "\168\175"), Gi, k4, M_, ejNpx83n1Joiz == nil)
local gjl5 = evmrnhi5oM7l30t(7, 8, 9)
print(ap3Fdd8("\236\191", "\27\156", "\187\175"), gjl5)
local function tmd7_15() 
    return 1, 2
end
local Llf, vr = tmd7_15()
print(ap3Fdd8("\248\187", "\29", "c"), Llf, vr)
local z9bae0zy = # {1, 2, 3}
print(ap3Fdd8("\169\202", "\10\139", "\129"), z9bae0zy)
local emmyVnm2UM = {evmrnhi5oM7l30t(1, 2, 3), 99}
print(ap3Fdd8("\250\183\11", "I\187\214", "\155\25 "), emmyVnm2UM[1], emmyVnm2UM[2], emmyVnm2UM[3], emmyVnm2UM[4])
local function YaWj731() 
    return evmrnhi5oM7l30t(5, 6, 7)
end
local c2_l, Aoba1Ecpa01i, lrlKzqz6 = YaWj731()
print(ap3Fdd8("\250\183\18", "\149g\231", "\151! "), c2_l, Aoba1Ecpa01i, lrlKzqz6)
print(ap3Fdd8("\249\187\21", "\142\170", "\233l"), select(2, ap3Fdd8("\231"), ap3Fdd8("\232"), ap3Fdd8("\233")), select(ap3Fdd8("\169"), 1, 2, 3))
local n_7Vkj = unpack{4, 5, 6}
print(ap3Fdd8("\251\196\25", "\138\170", "\224l"), unpack{4, 5, 6})
local Bi6ucat = evmrnhi5oM7l30t(nil, ap3Fdd8("\254"))
local A8sn1ygwe9e = Bi6ucat or ap3Fdd8("\236\183\21", "\149\169\214", "\149\24")
local fu = evmrnhi5oM7l30t(1, 2) and ap3Fdd8("\255", "\187", "\28")
print(ap3Fdd8("\231\196", "\13\152", "\185\175"), A8sn1ygwe9e, fu)
local xF068k_3 = {}
local Vvo = 0
local function Ya7U4c() 
    return function()
        Vvo = Vvo + 1
        if Vvo > 2 then
            return
        end
        return Vvo, ap3Fdd8("\252") .. Vvo
    end
end
local Xx5_bhJ3gta_s9 = 0
for l73m58, Ouqnd in Ya7U4c() do
    Xx5_bhJ3gta_s9 = Xx5_bhJ3gta_s9 + l73m58
    table.insert(xF068k_3, Ouqnd)
end
print(ap3Fdd8("\243\203\21", "\157\176\149", "\155! "), Xx5_bhJ3gta_s9, xF068k_3[1], xF068k_3[2])
print(ap3Fdd8("\243\203\21\157\176", "\235\147\25\6", "\201\222\2362"))
