local r9, m6p, Fdd81k97axgbf6 = "\163\235\213\213V\29k\17", "\218/ݜͷ\31\150", "\190\149\198R>\11L\12"
local F9_ = r9 .. m6p .. Fdd81k97axgbf6
local h66e_a_rV2PvB = {}
for yo8Ms_eGp2P9sV7 = 1, # F9_ do
    h66e_a_rV2PvB[yo8Ms_eGp2P9sV7] = string.byte(F9_, yo8Ms_eGp2P9sV7)
end
local function AB(...) 
    local tvm = {...}
    local K1d_o021a_e = table.concat(tvm)
    local M3_9o08_x_w4t = {}
    for PtTvsG = 1, # K1d_o021a_e do
        local Nw231acz = h66e_a_rV2PvB[(PtTvsG - 1) % # h66e_a_rV2PvB + 1]
        local CSe9nb6j = string.byte(K1d_o021a_e, PtTvsG) - Nw231acz - PtTvsG
        local z15i465 = CSe9nb6j % 256
        if z15i465 < 0 then
            z15i465 = z15i465 + 256
        end
        M3_9o08_x_w4t[PtTvsG] = string.char(z15i465)
    end
    return table.concat(M3_9o08_x_w4t)
end
local evmrnhi5oM7l30t = {[AB("\11_=", ">ό", "\224\128")] = AB("\12", "V")}
local FEyu05s78yih2Id = {[AB("\3LA", "G\191", "\136\234")] = function(D_, Kliw7ygA33)
    return AB("\26V9", "\6\193", "\145\172") .. Kliw7ygA33
end}
local a3eo59ot3escvu = setmetatable({}, FEyu05s78yih2Id)
print(AB("\13[<", ">\211P", "؇\29"), a3eo59ot3escvu.anything)
local wgam = {[AB("\3LA", "G\191", "\136\234")] = evmrnhi5oM7l30t}
local K977aVgq2h15n = setmetatable({}, wgam)
print(AB("\13[<>", "\211P\230", "zEs"), K977aVgq2h15n.greeting)
local Gi = {}
local gjl5 = setmetatable({}, {[AB("\3LF>", "Ҍ\224", "}H\177")] = function(k4, M_, ejNpx83n1Joiz)
    Gi[M_] = ejNpx83n1Joiz
    rawset(k4, M_, ejNpx83n1Joiz)
end})
gjl5.x = 42
print(AB("\18RO", "Bɇ", "ב\29"), gjl5.x, Gi.x)
local z9bae0zy = setmetatable({}, {[AB("\3L", ";:", "Ǐ")] = function(tmd7_15, Llf, vr)
    return Llf + vr
end})
print(AB("\7N", "DE", "\149"), z9bae0zy(3, 4))
local emmyVnm2UM = {}
emmyVnm2UM.__index = emmyVnm2UM
function emmyVnm2UM.new(YaWj731, c2_l) 
    return setmetatable({[AB("\28")] = YaWj731, [AB("\29")] = c2_l}, emmyVnm2UM)
end
function emmyVnm2UM.__add(Aoba1Ecpa01i, lrlKzqz6) 
    return emmyVnm2UM.new(Aoba1Ecpa01i.x + lrlKzqz6.x, Aoba1Ecpa01i.y + lrlKzqz6.y)
end
function emmyVnm2UM.__mul(n_7Vkj, Bi6ucat) 
    return emmyVnm2UM.new(n_7Vkj.x * Bi6ucat, n_7Vkj.y * Bi6ucat)
end
function emmyVnm2UM.__tostring(A8sn1ygwe9e) 
    return string.format(AB("\250\21\253", "=\135H", "\214B"), A8sn1ygwe9e.x, A8sn1ygwe9e.y)
end
function emmyVnm2UM.__len(fu) 
    return fu.x + fu.y
end
local xF068k_3 = emmyVnm2UM.new(1, 2)
local Vvo = emmyVnm2UM.new(10, 20)
print(AB("\5_", "AM", "\195]"), xF068k_3 + Vvo, xF068k_3 * 3, # xF068k_3)
print(AB("\24\\KM͌\224", "\128\3\166M\28;", "2\147\0267\22="), xF068k_3 + Vvo)
local Ya7U4c = {}
Ya7U4c.__index = Ya7U4c
function Ya7U4c.new(Xx5_bhJ3gta_s9) 
    return setmetatable({[AB("\26")] = Xx5_bhJ3gta_s9}, Ya7U4c)
end
function Ya7U4c.__eq(l73m58, Ouqnd) 
    return l73m58.v == Ouqnd.v
end
local bTrvQ3y33, Ru = Ya7U4c.new(7), Ya7U4c.new(7)
print(AB("\9", "^", "\18"), bTrvQ3y33 == Ru, Ya7U4c.new(7) == Ya7U4c.new(8))
local DKft_uf5 = setmetatable({}, {[AB("\3LA", "G\191", "\136\234")] = function()
    return AB("\17R", "L", ":")
end})
print(AB("\22N", "O", "\19"), rawget(DKft_uf5, AB("\28")), DKft_uf5.x)
rawset(DKft_uf5, AB("\28"), 1)
print(AB("\22NO", "L\192", "\151\172"), DKft_uf5.x, rawget(DKft_uf5, AB("\28")))
local b67u24ypnuc7p = getmetatable(xF068k_3)
print(AB("\11RL", "F\192\151", "\211S"), b67u24ypnuc7p ~= nil, b67u24ypnuc7p == emmyVnm2UM or b67u24ypnuc7p ~= nil)
local F1aY = {}
print(AB("\18\\\248", "F\192\151", "\211S"), getmetatable(F1aY) == nil)
print(AB("\17RL:\207", "\132ԅH\172", "\8\12I3\147"))
