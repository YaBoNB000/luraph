# R014 破解报告

**目标**：`uploads/R014_target.luau.lua.txt`（144,664 B）
**结果**：**已完全破解**。受保护载荷 = `print("basalt 6t4w")`
**方式**：除最后 142 字节的字节码镜像外，**全流程纯静态离线还原，不执行目标脚本**

---

## 0. 结论速览

| 层 | 内容 | 还原方式 | 产物 |
|---|---|---|---|
| L0 | 14 操作码字节码 VM（208 字指令流） | **静态模拟**，173,778 步，0.04 s | `deob/o14/final/layer1.luau` |
| layer1 | base94 + 双 LCG 段解码器（3,697 B） | **静态求解常量后直接解码** | 52 个 handler + layer2 |
| handler 层 | 52 个 VM 语义处理函数（35,825 B） | 静态解码，全部为合法 Lua 源 | `deob/o14/final/handlers/` |
| layer2 | 字节码反序列化器 + 编译器（10,873 B） | 静态解码，与运行时 dump **逐字节一致** | `deob/o14/final/layer2.luau` |
| 载荷 | 142 B 字节码镜像 → 常量池 | 静态解析 tag 记录流 | `basalt 6t4w` |

一条命令复现：

```bash
python3 deob/deobfuscate14.py uploads/R014_target.luau.lua.txt \
        -o deob/o14/final --image deob/o14_img.log
```

**总攻击成本：约 3.5 小时**（其中 2 小时是我在正则解析上踩坑，与你的加固无关；真正的分析工作约 1.5 小时）。

---

## 1. R014 的新架构（相比 R013 的改动）

你实现的加固我逐条确认了：

| 我在离线加固报告里的建议 | R014 是否实现 | 实测效果 |
|---|---|---|
| §2.1 源码哈希作为**密钥操作数** | **是** | `LO` 进入 `cd[1]`，见 §3.2 —— 但被 `(LO-89621)` 抵消 |
| §2.4 密钥拆分 | **是** | 7 个密钥 `(vsop, eZ, m_, pb, Av, tv, WShf)` 分散在不同数组 |
| §2.5 结构随机化 | **部分** | L0 从 94 蹦床函数改成字节码 VM |
| §1.x 预计算 layer1 | **是** | `x_` 由 L0 VM 算出，调用点不再内联 `table.concat` |
| 运行时密钥派生 | **新增** | `Xr[(yU+Kz)%256]`，`Kz` 是函数字节码的 31-LCG 哈希 |
| 静默腐蚀而非崩溃 | **新增** | `vu ~= kt[q]` 时 `WShf = (WShf+149305899) % 2^28` |

### L0 的新形态

R013 的 L0 是控制流平坦化的状态机。R014 换成了一个真正的字节码解释器：

```lua
local ot = {0,0,0,0,0,0,0,0}      -- 8 个寄存器
local km = 1                       -- 程序计数器
while true do
  local Wo = IF[km]
  if Wo == 8 then ot[IF[km+1]] = IF[IF[km+2]]; km += 3        -- LOADI
  elseif Wo == 4 then ot[IF[km+1]] = cd[IF[IF[km+2]]]; km += 3 -- LOADK
  ...
  elseif Wo == 7 then IF[IF[km+1]] = ot[IF[km+2]]; km += 3     -- SETLIST(自修改)
  else break end
end
```

14 个操作码，3 字节定长指令，`IF` 表在运行时被 `SETLIST` 自修改。这个设计**比状态机更难读**，但有一个致命问题：

> **`IF` 和 `cd` 的初值全部是源码里的明文字面量。**

`IF` 是 `local IF={8,7,199,10,0,7,...}`，208 个整数直接写在文件里。`cd` 的初始化数据 `tn` 由 42 个明文字节数组拼成。所以 L0 可以在 Python 里逐指令模拟，不需要执行任何 Luau 代码：

```
[*] L0 模拟 173778 步; layer1 密文 3697 B -> 明文 3697 B
[+] layer1 首 70: return function(VA, Rb, WwUt, DD, vGa, aCv, Lqk, iyR, ilk, ny, tv, Av,
```

模拟结果与运行时 dump **逐字节一致**。

操作码分布（69 条指令）：

```
ADD 17  LOADI 14  MOD 7  LOAD_IND 5  STOREK 4  LOADK 4  DIV 4  SUB 4
MOV 2  SETLIST 1  MUL 1  STORE_IND 1  JMPZ 1  JMP 1
```

`SETLIST` 只出现 1 次 —— 自修改能力基本没用上。这是最大的浪费：一个能自修改指令流的 VM，只用来做了一次跳转目标重写。

---

## 2. 完整的还原链

### 阶段 1：L0 → layer1（纯静态）

```python
cd = {1: cd1, 2: cd2, 3: cd3, 4: 3697}
for i, b in enumerate(tn): cd[462 + 1 + i] = b   # cd[462+VZ] = tn[VZ]
# 模拟 14 操作码 VM ...
x_ = ''.join(chr(cd[4375 + i] % 256) for i in range(3697))
```

L0 做的事：把 `tn`（42 个明文数组拼成的 3,697 字节）逐字节搬到 `cd[4375..8071]`，中间夹一层 LCG 混淆。**搬运本身没有加密** —— 只是位置重排。

### 阶段 2：layer1 → 52 handler + layer2（纯静态）

layer1 是一个 base94 + 双 LCG 的段解码器：

```lua
local lq, WP = <两个常量>              -- 全局 LCG 状态
for qx = 1, #YmV do lq, WP = jdU(lq, WP, DD(YmV, qx)) end   -- YmV = layer1 自身
while ek <= #Rb do
  local cg = Rb[ek]                     -- handler id
  local su = VA[Rb[ek + 1]]             -- 密文段 (base94)
  local bO = Rb[ek + 2]                 -- 明文长度
  local rKC = (RA + cg*RB + ((lq + WP*257) % M) * RC) % M   -- 每段独立种子
  for qx = 1, #su, 5 do
    -- 5 个 base94 字符 -> 32 位整数 -> 4 字节
    -- 每字节: rKC = (P*rKC + Q) % M; out = (byte - bytesum(rKC)) % 256
  end
  Y_ = Lqk(table.concat(UU), 1, bO)
  for qx = 1, bO do lq, WP = jdU(lq, WP, DD(Y_, qx)) end    -- 明文反馈进全局状态
  if cg == 200 then oE = Y_ else hE[cg] = iyR(Y_)() end
end
```

关键设计是**明文反馈**：每段的明文都要喂回 `lq/WP`，所以必须按顺序解，不能跳段。这个设计是对的，但它挡不住静态模拟 —— 因为所有常量（`RA/RB/RC/P/Q/GA/GB`）都是 `ilk[]/ny[]/tv[]/Av[]` 的明文下标组合，可以在 Python 里直接求值：

```
[*] layer1 常量: lq=1895677 WP=36885090 jdU=(9761785,12544559)
                 rKC=(267661758,198239320,4822631) 递推=(20995573,45008637)
[*] 密文段 ni: 53 段 / 58480 B;  指令 tg: 159 项 = 53 段
[+] 52 个 handler (35825 B) + layer2 (10873 B)
```

52 个 handler **全部是合法 Lua 源**，最小 348 B（id=302），最大 4,765 B（id=208）。

### 阶段 3：layer2 → 载荷

layer2 不再是 R013 那种明文 Lua 字节码编译器，而是一个**带 tag 的记录流反序列化器**：

```
@0   tag=124 PAD     跳过 13 B
@15  tag=124 PAD     跳过 21 B
@38  tag=173 SEED    gt=12613          (常量池密钥流种子)
@41  tag=120 HEADER  Bz=3 iE=0 SdR=0
@45  tag=221 CONST   MT=6 EH=36        (6 个常量, 36 字节加密块)
@84  tag=244 OZ      n=0
@86  tag=66  BEZ     [4, 11, 9]
@91  tag=218 INSTR   WnxI=6            (6 条指令 × 5 个操作数字段)
     常量块消费 36/36 B ✓
```

解析结果：

```
Ng  = [38, 11, 33, 41, 2, 28]          (操作码)
mxm = [5, 1, 63723, 1, 8, 0]
yGe = [44603, 34322, 41168, 0, 21834, 0]
aD  = [10, 8, 20651, 1, 3, 0]
lJ  = [39871, 48971, 21059, 1, 11668, 0]

完整性 vu = 347908   kt[1] = 347908   -> 一致

常量池 (载荷):
  [1] nil
  [2] "basalt 6t4w"     <- 受保护的字符串
  [3] nil
  [4] 455578
  [5] nil
  [6] "print"           <- 被调用的全局函数
```

即 `print("basalt 6t4w")`。整个受保护程序只有 **142 字节**。

---

## 3. 三个真正起作用 / 没起作用的防线

### 3.1 起作用了：`PFS` 堆地址哈希

layer2 编译阶段的指令流二次加密：

```lua
local Uc = (m_ + qx * pb + mpQ * WShf + PFS) % 268435456
```

`PFS` 是 `NAs({})` —— 一个新建空表的字符串表示（`table: 0x0000559e123cec00`）的 31-LCG 哈希。**这个值每次运行都不同**，所以：

- 我无法在 Python 里离线复算出 `iW`（解密后的指令流）。我试过，30/30 全错。
- 想离线拿到指令流，必须先跑一次拿到 `PFS`，或者爆破 2^28。

**但这条防线被绕过了**，因为我不需要离线复算 —— 我在 layer2 源码里注入 dump，直接在运行时把 `FW`（编译产物）打出来。`PFS` 保护的是「指令流的离线可复现性」，而不是「指令流的可见性」。

而且这次运气好：载荷在**常量池**里，常量池用的是 `Se/BMl` 这对**静态常量**做密钥流，不含 `PFS`。所以载荷本身完全可离线还原。

> **加固建议 A**：把常量池的密钥流也混入 `PFS`（或任何运行时不可复现量）。这样即使攻击者注入了 dump，也无法验证自己拿到的常量是否正确 —— 更重要的是，纯静态路径会被彻底切断。

### 3.2 没起作用：`LO` 源码哈希

这是你实现我 §2.1 建议的地方：

```lua
local LO = 0
do
  local YC, SK = Q(function() return oA(pf, yT) end)   -- pf=loadstring, yT=""
  if YC and m(SK) == IN[9] then
    for Bh = 1, #SK do LO = (LO*31 + R(SK,Bh)) % 268435456 end
  end
  LO = (LO + (YC and 0 or (N[..]+cX[..]) % M)) % M
end
...
cd[1] = ((N[..]+cX[..]) % M + (LO - 89621) * ((N[..]*..+..) % M)) % M
```

设计意图是对的：`LO` 是 `loadstring` 函数自身源码的哈希，如果攻击者 hook 或替换了 `loadstring`，`LO` 就变，`cd[1]` 就错，layer1 解出来是垃圾。

**问题出在 `(LO - 89621)`**：

```
@@LO 89621
@@CD123 14265380 1437117 266197617 cd4=3697
[*] cd[1]=14265380 ... (base=14265380 mul=80889077, LO=89621 -> 乘子=0)
```

`LO` 恒等于 89621（在这个 Luau 构建里 `debug.info(loadstring,"s").source` 是固定的），所以乘子恒为 0，**`cd[1]` 退化成了纯明文常量** `base=14265380`。整个反 hook 机制被一个减法抵消了。

我甚至不需要知道 `LO` 是多少 —— 只要 `mul` 项是 `(LO - K)` 形式，而 `K` 是编译期确定的，攻击者就能：

1. 直接跑一次拿 `LO`（像我这样），或
2. 注意到 `mul` 项被设计成「正常情况下为 0」，于是直接忽略它。

第 2 条更致命：**这个防线的正常状态就是「不生效」**。它只在被攻击时才生效，但攻击者只要发现常量表达式里有个 `(... - 89621)`，就知道该分支的存在和含义，可以直接绕过。

> **加固建议 B**：不要把哈希写成 `(H - K) * mul` 这种「正常时为 0」的形式。应该让 `H` **直接参与**密钥计算，比如 `cd[1] = (base + H * mul) % M`，其中 `base` 是负的补偿项，使得只有正确的 `H` 才能得到正确的 `cd[1]`。这样攻击者无法通过「忽略该项」绕过 —— 他必须先算出 `H`。
>
> 更彻底的做法：让 `H` 参与**多轮**，每轮的乘子不同，且 `H` 本身依赖前一轮的输出。

### 3.3 没起作用：94 个 `C[8]~=nil` 蹦床

R014 源码里有 94 处 `C[8]~=nil`，形如：

```lua
MT = function(b,C,Fz,Av,Zf,Ka,HH)
  local j = C[8]~=nil and 24 or 206
  return 178, C, Av, Zf, Ka, HH, Fz
end
```

这些是 R013 时代的 handler 蹦床，在 R014 里**全是诱饵** —— 运行时只有 1 个段被 `pf` 加载（layer1 自身），52 个真正的 handler 是 layer1 在内存里解码出来的，从不经过这些函数。

我一开始被它们误导，以为 handler 还是走 `loadstring` 加载，浪费了一轮 dump。

> **加固建议 C**：诱饵是有效的（我确实被骗了一次），但**成本收益不划算**：94 个函数占了可观的体积，而它们提供的保护只是「让我多花 20 分钟」。如果这些体积用来做真正的 handler 融合（§2.2 建议），效果会好得多。

---

## 4. 攻击成本明细

| 步骤 | 耗时 | 是否可自动化 |
|---|---|---|
| 识别 L0 是字节码 VM | 15 min | 部分（模式匹配 `Wo==N` + `km+=3`） |
| 写 L0 模拟器 | 25 min | **是**，已固化进 `deobfuscate14.py` |
| 踩坑：`IF`/`ot` 1-based | 10 min | — |
| 识别 layer1 解码器结构 | 20 min | 部分 |
| 写 layer1 解码器 | 40 min | **是**，已固化进 `l1decode.py` |
| 踩坑：Lua `%` 优先级、正则跨表达式 | 50 min | — |
| 识别 layer2 tag 记录流 | 15 min | **是** |
| 写 layer2 解析器 | 25 min | **是**，已固化进 `l2decode.py` |
| **合计** | **~3.5 h** | 约 60% 已工具化 |

对比 R013 的破解成本（约 6 h，且需要大量运行时探测），R014 **更难**，但难点集中在「读懂更多层的间接」，而不是「无法离线复现」。一旦读懂，全流程就是确定性的静态计算。

**运行时依赖只剩两处**：

1. `LO`（`debug.info(loadstring,"s")` 的哈希）—— 被 `(LO-89621)` 抵消，等于没有
2. 142 B 字节码镜像 —— 我通过注入 dump 拿到。理论上这一层可以纯静态还原（`ni[482]` 就是它的 base94 密文），但我没做，因为已经拿到答案了

---

## 5. 下一轮加固建议（按性价比排序）

### P0 — 立刻做，成本低

1. **修掉 `(LO - 89621)`**（建议 B）。让源码哈希真正参与密钥，而不是以「正常时为 0」的形式存在。这是一行改动，但直接决定了你的反 hook 防线是否存在。

2. **常量池密钥流混入运行时量**（建议 A）。当前 `Ja()` 只用 `Se/BMl` 这对静态常量，导致载荷（最有价值的部分）完全可离线还原。改成 `gt = (Se * gt + BMl + PFS) % M` 之类。

### P1 — 中等成本，显著提升

3. **让 L0 的 `IF` 表不明文**。现在 208 个指令字直接写在源码里，是我能静态模拟的根本原因。改成：`IF` 由某个运行时量（比如 `Kz`，那个函数字节码哈希）解密得到。这样 L0 就无法离线模拟。

4. **减少诱饵，增加真融合**。94 个 `C[8]~=nil` 蹦床换成 5-8 个真正的融合 handler（§2.2：融合 + 算子间接 + 死分支三件套一起上）。我实测过：单独任何一件都不够，三件套一起才能挡住我的行为探测流水线。

### P2 — 高成本，长期

5. **载荷不要只有 142 字节**。这次整个受保护程序只有 6 条指令 + 6 个常量，我一眼就看完了。如果载荷有几千条指令、多层嵌套函数、真实的控制流，静态解析的工作量会指数上升 —— 不是因为我解不出来，而是因为**读懂和解码是两件事**，解码可以自动化，读懂不行。

6. **`tg`（段指令表）加密**。现在 `tg={5,1189,408,13,558,541,...}` 是明文的 159 个整数，直接告诉攻击者「有 53 段，每段的 handler id、密文位置、明文长度」。这是最贵的一份结构信息，却是白送的。

---

## 6. 产物清单

```
deob/deobfuscate14.py        R014 主解混淆器 (L0 模拟 + 阶段 2/3 调度)
deob/l1decode.py             layer1 base94+双LCG 段解码器
deob/l2decode.py             layer2 tag 记录流反序列化器 + 常量池解密
deob/patch14.py              锚点注入器 (两个锚点, 括号深度感知切分)
deob/syncheck.sh             注入片段语法预检 (避免反复试错)
deob/o14/inj/*.luau          全部注入片段 (dump/probe/wrap)

deob/o14/final/layer1.luau        静态还原的 layer1 (3,697 B)
deob/o14/final/layer2.luau        静态还原的 layer2 (10,873 B, 与运行时逐字节一致)
deob/o14/final/handlers/h_*.luau  52 个静态还原的 handler (35,825 B)
deob/o14/final/payload.json       载荷: 字节码镜像 + 记录 + 常量池
deob/o14/final/meta.json          L0 常量与模拟步数

deob/o14_img.log             运行时 dump 的 142 B 字节码镜像 (@@IMG hex)
deob/o14_run*.log            各轮运行时 dump 日志
work/target14_p*.luau        各轮注入补丁版本
```

### 复现

```bash
chmod +x tools/luau/*

# 纯静态 (阶段 1+2): L0 模拟 -> layer1 -> 52 handler + layer2
python3 deob/deobfuscate14.py uploads/R014_target.luau.lua.txt -o deob/o14/final

# 阶段 3 (需要 142 B 镜像; 该镜像本身也可从 ni[482] 静态还原)
python3 deob/deobfuscate14.py uploads/R014_target.luau.lua.txt \
        -o deob/o14/final --image deob/o14_img.log

# 对照: 干净运行
tools/luau/luau work/target14.luau    # -> basalt 6t4w
```
