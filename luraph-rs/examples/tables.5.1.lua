local QupUc, Og, j97yFcw7 = "G\158\20L\150\230\251|", "\131\28\9ov\164R\241", "\159\196dC\215\206\236\135"
local Kj8_7i8i_atH88s = QupUc .. Og .. j97yFcw7
local tygwe9ec = {}
for udxF068k = 1, # Kj8_7i8i_atH88s do
    tygwe9ec[udxF068k] = string.byte(Kj8_7i8i_atH88s, udxF068k)
end
local function FdY975C68du21u(...) 
    local SbhJ3 = {...}
    local Ta_ = table.concat(SbhJ3)
    local ttl7 = {}
    for m580OuqndIbTrvQ = 1, # Ta_ do
        local y33x5ur9 = tygwe9ec[(m580OuqndIbTrvQ - 1) % # tygwe9ec + 1]
        local ft_uf55_l6 = string.byte(Ta_, m580OuqndIbTrvQ) - y33x5ur9 - m580OuqndIbTrvQ
        local hwrorv96753n = ft_uf55_l6 % 256
        if hwrorv96753n < 0 then
            hwrorv96753n = hwrorv96753n + 256
        end
        ttl7[m580OuqndIbTrvQ] = string.char(hwrorv96753n)
    end
    return table.concat(ttl7)
end
local evmrnhi5oM7l30t = {1, 2, 3}
print(FdY975C68du21u("\169\18", "\137\177", "\20&"), evmrnhi5oM7l30t[1], evmrnhi5oM7l30t[3], # evmrnhi5oM7l30t)
local D_ = {[FdY975C68du21u("\169")] = 1, [FdY975C68du21u("\170")] = 2, [FdY975C68du21u("\171", "\192", "{")] = 3}
print(FdY975C68du21u("\176\1", "\138\184", "\213"), D_.a, D_.b, D_[FdY975C68du21u("\171", "\192", "{")])
local Kliw7ygA33 = {1, [FdY975C68du21u("\192")] = 5, [1 + 1] = 9, [3] = {7, 8}}
print(FdY975C68du21u("\181\9", "\143\181", "\255&"), Kliw7ygA33[1], Kliw7ygA33.x, Kliw7ygA33[2], Kliw7ygA33[3][1], Kliw7ygA33[3][2])
local FEyu05s78yih2Id = {}
print(FdY975C68du21u("\173\13", "\135\196", "\20&"), # FEyu05s78yih2Id, next(FEyu05s78yih2Id) == nil)
local a3eo59ot3escvu = {}
for wgam = 1, 3 do
    a3eo59ot3escvu[wgam] = {}
    for K977aVgq2h15n = 1, 3 do
        a3eo59ot3escvu[wgam][K977aVgq2h15n] = wgam * 10 + K977aVgq2h15n
    end
end
print(FdY975C68du21u("\175\18", "\128\180", "\213"), a3eo59ot3escvu[2][3], a3eo59ot3escvu[3][1])
local Gi = {5, 3, 8}
table.insert(Gi, 1, 9)
print(FdY975C68du21u("\177\14\138", "\181\13", "`<"), Gi[1], # Gi)
local k4 = table.remove(Gi, 2)
print(FdY975C68du21u("\186\5\132", "\191\17", "Q<"), k4, Gi[2])
local M_ = {3, 1, 2}
table.sort(M_)
print(FdY975C68du21u("\187\15", "\137\196", "\213"), M_[1], M_[2], M_[3])
local ejNpx83n1Joiz = table.concat({FdY975C68du21u("\169"), FdY975C68du21u("\170"), FdY975C68du21u("\171")}, FdY975C68du21u("u"))
print(FdY975C68du21u("\171\15\133", "\179\252", "`<"), ejNpx83n1Joiz, # table.concat({1, 2}, FdY975C68du21u("")))
local gjl5 = FdY975C68du21u("h\192\127", "\181\7X", "q\164\172")
print(FdY975C68du21u("\181\5\139", "\184\10", "P<"), gjl5:upper():sub(4, 8))
local tmd7_15 = 0
for Llf, vr in ipairs{10, 20, 30, 40} do
    tmd7_15 = tmd7_15 + Llf * vr
end
print(FdY975C68du21u("\177\16x", "\185\13", "_<"), tmd7_15)
local function z9bae0zy(emmyVnm2UM) 
    return {[FdY975C68du21u("\190", "\1", "\131")] = emmyVnm2UM, [FdY975C68du21u("\187", "\17")] = emmyVnm2UM * emmyVnm2UM}
end
local YaWj731 = z9bae0zy(9)
print(FdY975C68du21u("\191\18x", "\192\15M", "d\190"), YaWj731.val, YaWj731.sq)
print(FdY975C68du21u("\188\1y\188", "\0_\"\232", "\251\148y"))
