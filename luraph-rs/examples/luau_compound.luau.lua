local zu, jl, Otmd7_15 = "\224\29\140\200)\155zo", "r\209Y\171\228J*G", "i9\244\253\183\137\143\142"
local Llf = zu .. jl .. Otmd7_15
local vr = {}
for z9bae0zy = 1, # Llf do
    vr[z9bae0zy] = string.byte(Llf, z9bae0zy)
end
local function emmyVnm2UM(...) 
    local YaWj731 = {...}
    local c2_l = table.concat(YaWj731)
    local Aoba1Ecpa01i = {}
    for lrlKzqz6 = 1, # c2_l do
        local n_7Vkj = vr[(lrlKzqz6 - 1) % # vr + 1]
        local Bi6ucat = string.byte(c2_l, lrlKzqz6) - n_7Vkj - lrlKzqz6
        local A8sn1ygwe9e = Bi6ucat % 256
        if A8sn1ygwe9e < 0 then
            A8sn1ygwe9e = A8sn1ygwe9e + 256
        end
        Aoba1Ecpa01i[lrlKzqz6] = string.char(A8sn1ygwe9e)
    end
    return table.concat(Aoba1Ecpa01i)
end
local evmrnhi5oM7l30t = 10
evmrnhi5oM7l30t = evmrnhi5oM7l30t + 5
print(emmyVnm2UM("B\131", "\243", "\6"), evmrnhi5oM7l30t)
local D_ = 10
D_ = D_ - 4
print(emmyVnm2UM("T\148", "\241", "\6"), D_)
local Kliw7ygA33 = 3
Kliw7ygA33 = Kliw7ygA33 * 4
print(emmyVnm2UM("N\148", "\251", "\6"), Kliw7ygA33)
local FEyu05s78yih2Id = 12
FEyu05s78yih2Id = FEyu05s78yih2Id / 2
print(emmyVnm2UM("E\136", "\5", "\6"), FEyu05s78yih2Id)
local a3eo59ot3escvu = 17
a3eo59ot3escvu = a3eo59ot3escvu % 5
print(emmyVnm2UM("N\142", "\243", "\6"), a3eo59ot3escvu)
local wgam = 2
wgam = wgam ^ 5
print(emmyVnm2UM("Q\142", "\6", "\6"), wgam)
local K977aVgq2h15n = {[emmyVnm2UM("Y")] = 1, [2] = 10}
K977aVgq2h15n.x = K977aVgq2h15n.x + 1
K977aVgq2h15n[2] = K977aVgq2h15n[2] - 3
print(emmyVnm2UM("J\141", "\2431", "\166\219"), K977aVgq2h15n.x, K977aVgq2h15n[2])
local Gi = 2
Gi = Gi + 3 * 4
print(emmyVnm2UM("F\151", "\255>", "h"), Gi)
local k4 = 100
k4 = k4 % (7 + 1)
print(emmyVnm2UM("F\151\255", ">N\14", "\240۵"), k4)
local M_ = 1
while M_ <= 4 do
    M_ = M_ + 1
end
print(emmyVnm2UM("M\142", "\254<", "h"), M_)
print(emmyVnm2UM("M\148\240AN\4", "\240\228\235J\217%", "Ux\157\198\232\176"))
