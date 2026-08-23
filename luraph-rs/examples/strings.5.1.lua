local L6e7j, vIyf246r, czrr0 = "\29:|\161G\216=i", "b73捭_\24", "\245\173ө\171\209S\161"
local x9_aWj73 = L6e7j .. vIyf246r .. czrr0
local Kc2_lT4oba1E = {}
for pa01i = 1, # x9_aWj73 do
    Kc2_lT4oba1E[pa01i] = string.byte(x9_aWj73, pa01i)
end
local function lrlKzqz6(...) 
    local n_7Vkj = {...}
    local Bi6ucat = table.concat(n_7Vkj)
    local A8sn1ygwe9e = {}
    for fu = 1, # Bi6ucat do
        local xF068k_3 = Kc2_lT4oba1E[(fu - 1) % # Kc2_lT4oba1E + 1]
        local Vvo = string.byte(Bi6ucat, fu) - xF068k_3 - fu
        local Ya7U4c = Vvo % 256
        if Ya7U4c < 0 then
            Ya7U4c = Ya7U4c + 256
        end
        A8sn1ygwe9e[fu] = string.char(Ya7U4c)
    end
    return table.concat(A8sn1ygwe9e)
end
local evmrnhi5oM7l30t = lrlKzqz6("\146\157ᮽS\179", "\229\208c\160S\253&", "ʖr\201K+$")
print(lrlKzqz6("\131\175\226", "\6\188C", "\183\171"), # evmrnhi5oM7l30t, evmrnhi5oM7l30t:sub(1, 3))
local D_ = lrlKzqz6("\131\162\230\233", "\145޾", "\214ݰ")
print(lrlKzqz6("\128\181\243", "\10lC", "\183ԥ"), # D_, D_:sub(1, 5), D_:sub(6, 6) == lrlKzqz6("\30"))
local Kliw7ygA33 = lrlKzqz6("\138\171\237\12", "VQ\184\227", "ԯ\165")
print(lrlKzqz6("\138\171", "\237\12", "\134"), # Kliw7ygA33, Kliw7ygA33:sub(1, 4))
local FEyu05s78yih2Id = lrlKzqz6("\131\173\159\0", "\167\254\173\223", "ު\162W")
print(lrlKzqz6("\138\171\237", "\12lC", "\181\171"), FEyu05s78yih2Id:sub(5, 6), # FEyu05s78yih2Id)
print(string.format(lrlKzqz6("C\160\172\202", "\191\11i", "\159\157\167"), 7, lrlKzqz6("\150"), 3.14159))
print(string.format(lrlKzqz6("ya", "\242", "\2"), lrlKzqz6("Ol", "\175", "\202")))
print(string.format(lrlKzqz6("C", "\173"), lrlKzqz6("\127", "^", "\225")))
local a3eo59ot3escvu = lrlKzqz6("\134\161\235\17", "\187\254\187\224", "ݭ\162")
print(lrlKzqz6("\145\177", "\225", "\223"), a3eo59ot3escvu:sub(7, 11), a3eo59ot3escvu:sub(1, 5), a3eo59ot3escvu:sub(- 5))
print(lrlKzqz6("\144\161", "\239", "\223"), string.rep(lrlKzqz6("\127", "\158"), 3), # string.rep(lrlKzqz6("\150"), 5))
print(lrlKzqz6("\128\181", "\243\10", "\134"), a3eo59ot3escvu:byte(1), a3eo59ot3escvu:byte(7), string.byte(lrlKzqz6("_")))
print(lrlKzqz6("\129\164", "\224\23", "\134"), string.char(72, 105))
print(lrlKzqz6("\132\165", "\237\9", "\134"), a3eo59ot3escvu:find(lrlKzqz6("\149\171", "\241\17", "\176")), a3eo59ot3escvu:find(lrlKzqz6("\138"), 4), a3eo59ot3escvu:find(lrlKzqz6("\140\171", "\239", "\10")) == nil)
print(lrlKzqz6("\139\157", "\243\8", "\180\24"), a3eo59ot3escvu:match(lrlKzqz6("Fa\246\208", "u\254l\150", "\226lg")), a3eo59ot3escvu:match(lrlKzqz6("L", "f", "\235")))
local wgam = lrlKzqz6("\127\158\226\214", "~\17\168\214", "\209us(")
print(lrlKzqz6("\139\157\243\8", "\180\254\178", "\230\216{"), wgam:match(lrlKzqz6("Fa", "\227\208", "u")), select(2, wgam:find(lrlKzqz6("Fa\227", "\208u\3", "\187\156"))))
local K977aVgq2h15n = 0
for Gi in lrlKzqz6("\141\170\228\197\192", "U\179\145\223", "\169\176W\255"):gmatch(lrlKzqz6("C", "\179", "\170")) do
    K977aVgq2h15n = K977aVgq2h15n + # Gi
end
print(lrlKzqz6("\133\169\224", "\25\175", "F~"), K977aVgq2h15n)
print(lrlKzqz6("\133\175", "\244\7", "\134"), lrlKzqz6("\127i", "\225\210", "\175"):gsub(lrlKzqz6("C", "i"), lrlKzqz6("I")), lrlKzqz6("\127", "\158", "\226"):gsub(lrlKzqz6("\128"), lrlKzqz6("v", "\149")))
local k4, M_ = lrlKzqz6("\134\161", "\235\17", "\187"):gsub(lrlKzqz6("\138"), lrlKzqz6("j"))
print(lrlKzqz6("\133\175\244\7", "lA\179\230", "ٵx"), k4, M_)
local ejNpx83n1Joiz = lrlKzqz6("\150") .. lrlKzqz6("\151") .. lrlKzqz6("\152")
print(lrlKzqz6("\129\171\237", "\8\173", "R~"), ejNpx83n1Joiz, # lrlKzqz6("\150") .. lrlKzqz6("\151"))
local gjl5 = lrlKzqz6("\2\244,\139", "\226e*&", "\246)\237\135")
print(lrlKzqz6("\147\176", "\229\221", "\134"), # gjl5, gjl5:sub(1, 2))
print(lrlKzqz6("\145\176\241\14", "\186E\183\145", "ϰ\172W"))
