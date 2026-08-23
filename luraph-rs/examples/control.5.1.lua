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
local function evmrnhi5oM7l30t(D_) 
    if D_ < 0 then
        return Damwac("\211", "~", "K")
    elseif D_ == 0 then
        return Damwac("\223~", "V", "\158")
    else
        return Damwac("\213", "\136", "W")
    end
end
print(evmrnhi5oM7l30t(- 1), evmrnhi5oM7l30t(0), evmrnhi5oM7l30t(5))
local Kliw7ygA33 = 3
if Kliw7ygA33 > 0 then
    if Kliw7ygA33 > 2 then
        print(Damwac("\199", "\130", "K"))
    end
end
local FEyu05s78yih2Id = 0
while FEyu05s78yih2Id < 4 do
    FEyu05s78yih2Id = FEyu05s78yih2Id + 1
end
print(Damwac("܁", "M\155", "\180C"), FEyu05s78yih2Id)
local a3eo59ot3escvu = 0
while true do
    a3eo59ot3escvu = a3eo59ot3escvu + 1
    if a3eo59ot3escvu == 3 then
        break
    end
end
print(Damwac("ǋ", "I\144", "\186C"), a3eo59ot3escvu)
local wgam = 10
repeat
    wgam = wgam - 3
until wgam <= 0
print(Damwac("\215~T", "\148\176", "}\233"), wgam)
local K977aVgq2h15n = 0
for Gi = 1, 5 do
    K977aVgq2h15n = K977aVgq2h15n + Gi
end
local k4 = 0
for M_ = 10, 1, - 2 do
    k4 = k4 + M_
end
local ejNpx83n1Joiz = 0
for gjl5 = 2, 20, 6 do
    ejNpx83n1Joiz = ejNpx83n1Joiz + gjl5
end
print(Damwac("ˈV", "\157\196", "v\233"), K977aVgq2h15n, k4, ejNpx83n1Joiz)
local tmd7_15 = {10, 20, 30}
local Llf = 0
for vr, z9bae0zy in ipairs(tmd7_15) do
    Llf = Llf + z9bae0zy
end
local emmyVnm2UM = {[Damwac("\198")] = 1, [Damwac("\199")] = 2}
local YaWj731 = 0
for c2_l, Aoba1Ecpa01i in pairs(emmyVnm2UM) do
    YaWj731 = YaWj731 + Aoba1Ecpa01i
end
print(Damwac("ˈV", "\150\180", "w\233"), Llf, YaWj731)
local function lrlKzqz6() 
    local n_7Vkj = 0
    return function()
        n_7Vkj = n_7Vkj + 1
        if n_7Vkj > 3 then
            return
        end
        return n_7Vkj, n_7Vkj * n_7Vkj
    end
end
local Bi6ucat = 0
for A8sn1ygwe9e, fu in lrlKzqz6() do
    Bi6ucat = Bi6ucat + fu
end
print(Damwac("ȎW\163", "\190v\207", "\148w\182"), Bi6ucat)
local xF068k_3 = 0
do
    for Vvo = 1, 3 do
        for Ya7U4c = 1, 3 do
            if Ya7U4c == 2 then
                break
            end
            xF068k_3 = xF068k_3 + 1
        end
    end
end
print(Damwac("\211~W\163\180", "mύu", "៑P"), xF068k_3)
do

end
if false then
    print(Damwac("\211~", "Z\148", "\193"))
end
print(Damwac("ȈR\163", "\193x\27K", "g묋"))
