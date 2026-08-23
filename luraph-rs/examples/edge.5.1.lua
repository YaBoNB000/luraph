local ttl7, m580OuqndIbTrvQ, y33x5ur9 = "d\23\225+J\3\168#", "\250r3\26\9\154\159t", "\29\129\172\9%\217\10\227"
local ft_uf55_l6 = ttl7 .. m580OuqndIbTrvQ .. y33x5ur9
local hwrorv96753n = {}
for p9zgtI9 = 1, # ft_uf55_l6 do
    hwrorv96753n[p9zgtI9] = string.byte(ft_uf55_l6, p9zgtI9)
end
local function Damwac(...) 
    local Cj = {...}
    local mz87w = table.concat(Cj)
    local Afiacs5v1ozh = {}
    for PXL166e_ = 1, # mz87w do
        local J4aests = hwrorv96753n[(PXL166e_ - 1) % # hwrorv96753n + 1]
        local dz = string.byte(mz87w, PXL166e_) - J4aests - PXL166e_
        local efe = dz % 256
        if efe < 0 then
            efe = efe + 256
        end
        Afiacs5v1ozh[PXL166e_] = string.char(efe)
    end
    return table.concat(Afiacs5v1ozh)
end
local function evmrnhi5oM7l30t() 

end
print(Damwac("ʆT", "\163\200)", "\21\153="), evmrnhi5oM7l30t() == nil)
local D_ = {}
print(Damwac("ʆT\163", "\200)#", "\140e\182"), # D_ == 0)
local Kliw7ygA33 = Damwac("")
print(Damwac("ʆT\163", "\200)\"", "\159u\182"), # Kliw7ygA33 == 0, Kliw7ygA33 == Damwac(""))
local function FEyu05s78yih2Id() 
    return
end
print(Damwac("\215~XO", "\189x#\147", "l\234\165`"), FEyu05s78yih2Id() == nil)
local a3eo59ot3escvu = a
print(Damwac("̅S\145\176u", "Ϟh\232\164", "\152{\9\18\190"), a3eo59ot3escvu == nil)
local wgam = 5
local K977aVgq2h15n = wgam
print(Damwac("Ȉ", "T\168", "\137"), wgam, K977aVgq2h15n)
print(Damwac("ڇ", "E\161", "\200C"), - - 5, not not true, not not true, # {1, 2})
print(Damwac("\213zV", "\148\189", "|\233"), (1 + 2) * (3 - 1))
local Gi = 0
local function k4() 
    Gi = Gi + 1
    return 1
end
local M_ = true or k4()
local ejNpx83n1Joiz = false and k4()
print(Damwac("؁S\161\195", "l\24\157f", "\241\167\154P"), M_ == true, ejNpx83n1Joiz == false, Gi == 0)
print(Damwac("\200zP", "\155\194}", "\28\159="), 1)
local gjl5 = 1
local tmd7_15 = 2
print(Damwac("\216~", "Q\152", "\137"), gjl5 + tmd7_15)
local Llf = 15
if Llf > 10 and Llf < 20 or Llf == 99 then
    print(Damwac("ȈQ\159\187n", "'Kf\235\172", "\138P\200\29\239"))
end
local vr = {3, 5, 7}
local z9bae0zy = 0
local emmyVnm2UM = 0
while true do
    z9bae0zy = z9bae0zy + 1
    if z9bae0zy > # vr then
        break
    end
    emmyVnm2UM = emmyVnm2UM + vr[z9bae0zy]
end
print(Damwac("܁M\155", "\180)\"\159", "h\236\177`"), emmyVnm2UM)
local YaWj731 = 1
repeat
    YaWj731 = YaWj731 * 2
until YaWj731 > 8 and YaWj731 < 32
print(Damwac("\215~T\148\176", "}ώr\233", "\174\146{ \232"), YaWj731)
local function c2_l() 
    local Aoba1Ecpa01i = 0
    local function lrlKzqz6(n_7Vkj) 
        Aoba1Ecpa01i = n_7Vkj
    end
    local function Bi6ucat() 
        return Aoba1Ecpa01i
    end
    lrlKzqz6(99)
    return Bi6ucat()
end
print(Damwac("\210z", "O\148", "\193C"), c2_l())
local A8sn1ygwe9e = {}
A8sn1ygwe9e.add = function(fu, xF068k_3)
    return fu + xF068k_3
end
A8sn1ygwe9e.sub = function(Vvo, Ya7U4c)
    return Vvo - Ya7U4c
end
print(Damwac("\215~K", "\163\176k", "\27\144="), A8sn1ygwe9e.add(10, 5), A8sn1ygwe9e.sub(10, 5))
print(Damwac("\202}K", "\148om", "\30\153h"))
