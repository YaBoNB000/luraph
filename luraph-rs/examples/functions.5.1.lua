local Pvf1aY, FjJfy_da, wacu_9u = "S\165\134}1\164D\205", "\228\250k\204\202(\187\4", "j\169\31\247\19\0\233\136"
local ap3Fdd8 = Pvf1aY .. FjJfy_da .. wacu_9u
local k97axg = {}
for f6vF = 1, # ap3Fdd8 do
    k97axg[f6vF] = string.byte(ap3Fdd8, f6vF)
end
local function DClAeh(...) 
    local J4aests = {...}
    local dz = table.concat(J4aests)
    local efe = {}
    for P2P9sV7l8Bztvm9 = 1, # dz do
        local bd_o021a_eWm3 = k97axg[(P2P9sV7l8Bztvm9 - 1) % # k97axg + 1]
        local Xa59x_w4tj_8 = string.byte(dz, P2P9sV7l8Bztvm9) - bd_o021a_eWm3 - P2P9sV7l8Bztvm9
        local But8f7xt49d9c = Xa59x_w4tj_8 % 256
        if But8f7xt49d9c < 0 then
            But8f7xt49d9c = But8f7xt49d9c + 256
        end
        efe[P2P9sV7l8Bztvm9] = string.char(But8f7xt49d9c)
    end
    return table.concat(efe)
end
local Kliw7ygA33 = function(evmrnhi5oM7l30t, D_)
    return evmrnhi5oM7l30t + D_
end
print(DClAeh("\181\21", "\248\239", "p"), Kliw7ygA33(2, 40))
local function FEyu05s78yih2Id() 
    local a3eo59ot3escvu = 0
    return function(wgam)
        a3eo59ot3escvu = a3eo59ot3escvu + (wgam or 1)
        return a3eo59ot3escvu
    end
end
local K977aVgq2h15n = FEyu05s78yih2Id()
print(DClAeh("\201\23", "\255\226", "\162\228"), K977aVgq2h15n(), K977aVgq2h15n(10), K977aVgq2h15n(1))
local Gi = {}
for k4 = 1, 3 do
    Gi[k4] = function()
        return k4
    end
end
print(DClAeh("\192\22\248\241V", "\13\172Ea", "y\232=\17"), Gi[1](), Gi[2](), Gi[3]())
local function M_(ejNpx83n1Joiz) 
    if ejNpx83n1Joiz <= 1 then
        return 1
    end
    return ejNpx83n1Joiz * M_(ejNpx83n1Joiz - 1)
end
local gjl5, tmd7_15
gjl5 = function(Llf)
    if Llf == 0 then
        return true
    end
    return tmd7_15(Llf - 1)
end
tmd7_15 = function(vr)
    if vr == 0 then
        return false
    end
    return gjl5(vr - 1)
end
print(DClAeh("\198\12", "\236", "\187"), M_(6), gjl5(10), tmd7_15(10), gjl5(11))
local function z9bae0zy(emmyVnm2UM) 
    if emmyVnm2UM == 0 then
        return DClAeh("\200\8\242", "\237c\14", "\186CR")
    end
    return z9bae0zy(emmyVnm2UM - 1)
end
print(z9bae0zy(5000))
local function YaWj731(c2_l, ...) 
    local Aoba1Ecpa01i = {...}
    return c2_l + # Aoba1Ecpa01i, select(DClAeh("w"), ...)
end
print(DClAeh("\202\8\251", "\226\168", "\17\133"), YaWj731(10, 1, 2, 3), YaWj731(1))
print(DClAeh("\202\8\251\226", "\168\17k6", "Yp\176"), unpack{5, 6, 7})
local fu = {[DClAeh("\181", "\11", "\237")] = function(lrlKzqz6, n_7Vkj)
    return lrlKzqz6 + n_7Vkj
end, [DClAeh("\199", "\28", "\235")] = function(Bi6ucat, A8sn1ygwe9e)
    return Bi6ucat - A8sn1ygwe9e
end}
print(DClAeh("\186\21\255", "\226\162\31", "\176\15"), fu.add(3, 4), fu.sub(9, 4))
local function xF068k_3() 
    return xF068k_3 ~= nil
end
print(DClAeh("\199\12\245", "\231\168\15", "\177\15"), xF068k_3())
local Vvo = {}
Vvo.x = 1
Vvo.y = 2
function Vvo:mag() 
    return self.x + self.y
end
Vvo.__add = function(Ya7U4c, Xx5_bhJ3gta_s9)
    return Ya7U4c + Xx5_bhJ3gta_s9
end
print(DClAeh("\193\12\253", "\233\165", "\14\133"), Vvo:mag())
local function l73m58(Ouqnd) 
    local bTrvQ3y33 = Ouqnd * 2
    local function Ru(DKft_uf5) 
        return bTrvQ3y33 + DKft_uf5
    end
    return Ru(10)
end
print(DClAeh("\194\12\252", "\245\155", "\14\133"), l73m58(5))
