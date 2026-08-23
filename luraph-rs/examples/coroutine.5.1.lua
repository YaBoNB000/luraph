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
local Kliw7ygA33 = coroutine.create(function(evmrnhi5oM7l30t, D_)
    return evmrnhi5oM7l30t + D_
end)
print(Damwac("؍E", "\163\196", "|\233"), coroutine.status(Kliw7ygA33))
local FEyu05s78yih2Id, a3eo59ot3escvu = coroutine.resume(Kliw7ygA33, 20, 22)
print(Damwac("\215~W", "\164\188", "n\233"), FEyu05s78yih2Id, a3eo59ot3escvu)
print(Damwac("؍E\163\196", "|όi", "\240\163\152P"), coroutine.status(Kliw7ygA33))
local K977aVgq2h15n = coroutine.create(function()
    local wgam = coroutine.yield(1)
    return wgam * 2
end)
local Gi, k4 = coroutine.resume(K977aVgq2h15n)
local M_, ejNpx83n1Joiz = coroutine.resume(K977aVgq2h15n, 21)
print(Damwac("ނ", "I\155", "\179C"), k4, ejNpx83n1Joiz, coroutine.status(K977aVgq2h15n))
local gjl5 = coroutine.create(function()
    coroutine.yield(Damwac("\198"), Damwac("\199"), Damwac("\200"))
    coroutine.yield(Damwac("\201"))
end)
local tmd7_15, Llf, vr, z9bae0zy = coroutine.resume(gjl5)
local emmyVnm2UM, YaWj731 = coroutine.resume(gjl5)
print(Damwac("ҎP\163", "\184)(\148", "h\232\162`"), Llf, vr, z9bae0zy, YaWj731)
local lrlKzqz6 = coroutine.create(function()
    local c2_l, Aoba1Ecpa01i = coroutine.yield()
    return c2_l + Aoba1Ecpa01i
end)
coroutine.resume(lrlKzqz6)
local n_7Vkj, Bi6ucat = coroutine.resume(lrlKzqz6, 3, 4)
print(Damwac("ҎP\163\184", ")!\144v", "\241\171\139P"), Bi6ucat)
local xF068k_3 = coroutine.create(function()
    local A8sn1ygwe9e = coroutine.wrap(function()
        coroutine.yield(Damwac("܋E", "\159\191", "n\19"))
    end)
    local fu = A8sn1ygwe9e()
    return Damwac("̈", "X", "i") .. fu
end)
local Vvo, Ya7U4c = coroutine.resume(xF068k_3)
print(Damwac("܋", "E\159", "\137"), Ya7U4c, coroutine.status(xF068k_3))
print(Damwac("\201~", "E\147", "\137"), coroutine.status(K977aVgq2h15n))
print(Damwac("ȈV\158\196", "}\24\153h\156", "\162\149\132\13"))
