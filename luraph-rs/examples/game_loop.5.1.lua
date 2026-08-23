local G6x1rns, tl73m580Ouqn, Ib = "\225e[*d\156d\243", "\160ZՌ\240\144d\23", "\225+J\3\168#\250r"
local RvQ3y33x5 = G6x1rns .. tl73m580Ouqn .. Ib
local r9Kft_u = {}
for v5_l6_u2 = 1, # RvQ3y33x5 do
    r9Kft_u[v5_l6_u2] = string.byte(RvQ3y33x5, v5_l6_u2)
end
local function Ypnuc(...) 
    local Pvf1aY = {...}
    local FjJfy_da = table.concat(Pvf1aY)
    local wacu_9u = {}
    for ap3Fdd8 = 1, # FjJfy_da do
        local k97axg = r9Kft_u[(ap3Fdd8 - 1) % # r9Kft_u + 1]
        local f6vF = string.byte(FjJfy_da, ap3Fdd8) - k97axg - ap3Fdd8
        local DClAeh = f6vF % 256
        if DClAeh < 0 then
            DClAeh = DClAeh + 256
        end
        wacu_9u[ap3Fdd8] = string.char(DClAeh)
    end
    return table.concat(wacu_9u)
end
local evmrnhi5oM7l30t = {}
evmrnhi5oM7l30t.state = {[Ypnuc("J", "\215")] = 100, [Ypnuc("N\204", "ԓ", "\213")] = 1, [Ypnuc("E\214", "ǜ", "\220")] = 0}
local function D_(Kliw7ygA33) 
    evmrnhi5oM7l30t.state.hp = evmrnhi5oM7l30t.state.hp - Kliw7ygA33
    if evmrnhi5oM7l30t.state.hp < 0 then
        evmrnhi5oM7l30t.state.hp = 0
    end
end
evmrnhi5oM7l30t.damage = D_
local FEyu05s78yih2Id = {}
local function a3eo59ot3escvu(wgam, K977aVgq2h15n) 
    local Gi = # FEyu05s78yih2Id + 1
    FEyu05s78yih2Id[Gi] = {[Ypnuc("G\221", "Ü", "\221")] = wgam, [Ypnuc("H", "\213")] = K977aVgq2h15n, [Ypnuc("E\214\204", "\156\206\5", "\223`\13")] = true}
    local k4 = {}
    function k4:Disconnect() 
        FEyu05s78yih2Id[Gi].connected = false
    end
    return k4
end
local function M_(ejNpx83n1Joiz, ...) 
    local gjl5 = 0
    for tmd7_15, Llf in ipairs(FEyu05s78yih2Id) do
        if Llf.event == ejNpx83n1Joiz and Llf.connected then
            Llf.fn(...)
            gjl5 = gjl5 + 1
        end
    end
    return gjl5
end
local vr = 0
local emmyVnm2UM = a3eo59ot3escvu(Ypnuc("J", "\208", "\210"), function(z9bae0zy)
    vr = vr + 1
    D_(z9bae0zy)
end)
local YaWj731 = a3eo59ot3escvu(Ypnuc("J", "\208", "\210"), function()
    evmrnhi5oM7l30t.state.coins = evmrnhi5oM7l30t.state.coins + 1
end)
M_(Ypnuc("J", "\208", "\210"), 10)
M_(Ypnuc("J", "\208", "\210"), 15)
YaWj731:Disconnect()
M_(Ypnuc("J", "\208", "\210"), 5)
print(Ypnuc("G\221\195", "\156\221", "\21\165"), vr, evmrnhi5oM7l30t.state.coins, evmrnhi5oM7l30t.state.hp)
local c2_l = 0
local function Aoba1Ecpa01i(lrlKzqz6) 
    c2_l = c2_l + lrlKzqz6
end
local n_7Vkj = 0
local function Bi6ucat(A8sn1ygwe9e) 
    while n_7Vkj < A8sn1ygwe9e do
        n_7Vkj = n_7Vkj + 1
        Aoba1Ecpa01i(0.1)
    end
end
Bi6ucat(50)
print(Ypnuc("N\214", "͞", "\163"), n_7Vkj, c2_l)
local Vvo = {[Ypnuc("U\219", "\191\162", "\206")] = Ypnuc("K\203", "\202", "\147"), [Ypnuc("Q\213", "\189\150", "\210\22")] = function(fu)
    fu.state = Ypnuc("J", "\208", "\210")
    return Ypnuc("J", "\208", "\210")
end, [Ypnuc("Qս\150", "\210\22\202_", "\24\210E")] = function(xF068k_3)
    xF068k_3.state = Ypnuc("K\203", "\202", "\147")
    return Ypnuc("K\203", "\202", "\147")
end}
print(Ypnuc("H\218", "\203", "h"), Vvo.on_hit(Vvo), Vvo.on_hit_done(Vvo))
return evmrnhi5oM7l30t
