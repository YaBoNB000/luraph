local F2, Luk3C70xR, T15jllf = ",\182\15\150\233\224\29\140", "\200)\155zor\209Y", "\171\228J*Gi9\244"
local vr = F2 .. Luk3C70xR .. T15jllf
local z9bae0zy = {}
for emmyVnm2UM = 1, # vr do
    z9bae0zy[emmyVnm2UM] = string.byte(vr, emmyVnm2UM)
end
local function YaWj731(...) 
    local c2_l = {...}
    local Aoba1Ecpa01i = table.concat(c2_l)
    local lrlKzqz6 = {}
    for n_7Vkj = 1, # Aoba1Ecpa01i do
        local Bi6ucat = z9bae0zy[(n_7Vkj - 1) % # z9bae0zy + 1]
        local A8sn1ygwe9e = string.byte(Aoba1Ecpa01i, n_7Vkj) - Bi6ucat - n_7Vkj
        local fu = A8sn1ygwe9e % 256
        if fu < 0 then
            fu = fu + 256
        end
        lrlKzqz6[n_7Vkj] = string.char(fu)
    end
    return table.concat(lrlKzqz6)
end
local evmrnhi5oM7l30t = YaWj731("\164'", "\132\6", "R")
local D_ = 41
print(string.format(YaWj731("\149\29~", "\6]\6", "I\7"), evmrnhi5oM7l30t))
print(string.format(YaWj731("\163\25~", "\15S ", "D\185D"), D_ + 1))
print(string.format(YaWj731("\161/\129\186\19Y", "D\245?\151ƫ", "\239\160E\215 "), D_, D_ * 2))
local Kliw7ygA33 = {[YaWj731("\165")] = 10}
print(string.format(YaWj731("\161\25t", "\6S\6", "I\7"), Kliw7ygA33.x))
print(YaWj731("\143*s\253", "S\6\159\180", "E\152\25\250"))
local FEyu05s78yih2Id = YaWj731("\151-\133\14\14", "GD\7E", "\165\15\244\227")
print(FEyu05s78yih2Id)
local function a3eo59ot3escvu(wgam, K977aVgq2h15n) 
    return wgam * K977aVgq2h15n
end
print(string.format(YaWj731("\144\25~", "\6\14", "\11\151"), a3eo59ot3escvu(3, 4)))
local Gi = {[YaWj731("\142")] = string.format(YaWj731("\142\245", "7", "\13"), 1 + 1), [YaWj731("\143")] = string.format(YaWj731("\143\245", "7", "\13"), string.upper(YaWj731("\165")))}
print(Gi.a, Gi.b)
print(string.format(YaWj731("\147%", "\134\186", "\19Y"), string.format(YaWj731("R", "\28"), 5)))
local k4 = 50
print(string.format(YaWj731("\157\27\134\186", "\19YD\248", "@\161\11"), k4))
print(YaWj731("\157\29\132\253ST", "\152\180=\156\26\235", "\238\225L\163\220") .. string.format(YaWj731("^\232", "B\191", "a^"), 1))
print(YaWj731("\153-s\15\14O", "\146\0086\165\22", "\166\224\239N\206"))
