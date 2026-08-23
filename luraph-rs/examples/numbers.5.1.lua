local Ltw, d8du, PV1M = "l&\163\159\164\18]\189", "Jp\185\151\202E4l", "\161\1608O\10K\198\226"
local a84_a3e = Ltw .. d8du .. PV1M
local v9 = {}
for T3e = 1, # a84_a3e do
    v9[T3e] = string.byte(a84_a3e, T3e)
end
local function cvu(...) 
    local wgam = {...}
    local K977aVgq2h15n = table.concat(wgam)
    local Gi = {}
    for k4 = 1, # K977aVgq2h15n do
        local M_ = v9[(k4 - 1) % # v9 + 1]
        local ejNpx83n1Joiz = string.byte(K977aVgq2h15n, k4) - M_ - k4
        local gjl5 = ejNpx83n1Joiz % 256
        if gjl5 < 0 then
            gjl5 = gjl5 + 256
        end
        Gi[k4] = string.char(gjl5)
    end
    return table.concat(Gi)
end
print(1 + 2, 7 - 10, 6 * 7, 10 / 4, 9 / 4)
print(7 % 3, - 7 % 3, 7 % - 3, - 1 % 3, 10 % 4)
print(2 ^ 32, 2 ^ 0.5, 4 ^ 0.5, 8 ^ (1 / 3))
print(0.1 + 0.2, 1 / 3, 100 / 10, 7.0 - 2)
print(math.floor(3.7), math.floor(- 3.7), math.abs(- 5), math.abs(5))
print(math.sqrt(16), math.max(3, 9, 2), math.min(4, 1, 8))
print(math.huge > 1e300, - math.huge < - 1e300)
print(math.pi > 3.14159)
print(16, 255, 11259375)
print(1e20, 15000000000.0, 2.5e-10 > 0)
print(tostring(1), tostring(1.5), tostring(1000000000), tostring(0.5))
print(tostring(1 / 3), tostring(2 ^ 40))
print(1 < 1.0, 1 == 1.0, - 1 == - 1)
print(5 + 1.5, 3 * 2.0)
local evmrnhi5oM7l30t = 9007199254740992.0
print(evmrnhi5oM7l30t, evmrnhi5oM7l30t - 9007199254740991.0)
print(math.pow(2, 10) == 2 ^ 10)
print(cvu("۝\19\5", "\14\138\215\229", "\183\2332\8"))
