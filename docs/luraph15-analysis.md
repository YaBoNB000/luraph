# Luraph v15.0 混淆脚本分析报告

> 样本：`samples/luraph15.txt`（171,753 字节，2 行，minified）
> 头部：`-- This file was protected using Luraph Obfuscator v15.0 [https://lura.ph/]`
> 分析日期：2026-08-23 ｜ 方法：静态结构分析（字符串/函数/别名/控制流模式提取）

---

## 1. 样本概况

| 项 | 值 |
|---|---|
| 体积 | 171.7 KB（其中 74.5 KB 为编码 blob 字面量） |
| 字符串字面量总数 | **仅 29 个**（全文件）——所有程序字符串都封在 blob 内 |
| handler 函数数 | **142 个**（`=function(b, ...)` 形态） |
| 目标环境 | **Roblox / Luau**（依赖 `buffer`、`Vector3.new`、`typeof`、`bit32`、`setfenv`） |
| 入口 | `return setmetatable(<模块表>, {}):FC()(...)` |

**最重要的形态变化（相对 v14）**：v15 的数据层从「Lua table 数组」换成了
**Roblox `buffer`（二进制内存）+ bit32 位运算 + 自定义 base-N 编码**。
样本无法在普通 Lua 5.1 / 纯 Luau 环境运行（无 buffer API）——
这是 Luraph 面向 Roblox 平台的专属构建。

---

## 2. 总体架构

```
模块表 T（setmetatable 的外层 metatable 为空 {}）
├── 142 个具名 handler 函数     —— opcode 处理 / 子分发器 / 工具
├── 数字别名槽 [N] → 原语函数    —— buffer.*/bit32.*/coroutine.*/string.*/
│                                  table.*/env/Roblox（数字索引隐藏真实 API 名）
├── RC = [=[LPH:...74.5KB...]    —— 主程序编码 blob（长字符串）
├── pC = {['"'] = "=W8+C", ...}  —— 10 个特殊字符 → 5 字符 token 的转义表
├── XC = "[ -'}~]"               —— 解码字符类（ASCII 32..126）
└── FC / L / M / _L ...          —— 顶层状态机与初始化器
```

### 2.1 入口与引导

```lua
return setmetatable(T, {}):FC()(...)
--                 ①        ②  ③
-- ① 模块表          ② FC(T) 引导状态机 → 返回入口函数 B   ③ B() 执行用户程序
```

- `_L` = 初始状态：`return true, 3, nil×10`（运行标志 + 起始状态 ID=3）
- `FC` = 顶层状态机：
  ```lua
  FC = function(b, ...)
    local u, ... = b:_L()            -- 初始 12 元状态
    local V, ... = ...               -- V = 状态 ID
    while u do
      if V<=1 then  V,... = b:qL(...)      -- 子分发器 qL
      elseif V<=3 then V,... = b:HL(...)   -- 子分发器 HL
      else
        q,... = b:eL(...)                 -- 子分发器 eL
        if q==2 then <继续状态机> elseif q==1 then return B end
      end
    end
  end
  ```
- `L` = 第二个顶层状态机（子分发器 KL/DL/JL/fL/kL/sL/aL…）——
  用于另一执行阶段（帧/协程执行器）
- `M` = 执行阶段初始化器：`return true, 19, nil...`

### 2.2 三层分发结构（v15 核心变化之一）

```
第 1 层：FC / L 顶层 while 状态机（3~4 个分支）
          ↓ 按状态 ID 调用
第 2 层：子分发器（qL/HL/eL/KL/DL/JL/… 每个自带 while + 二叉决策树）
          ↓ 按状态 ID 调用
第 3 层：142 个细粒度 handler（每个 = 小决策树，处理 3~4 个 opcode ID，
          执行一个操作后返回 (nextId, 状态元组...)）
```

- 状态通过**位置参数元组**传递（`b:qL(l, _, j, k, m, V, Y, d, g)`），
  handler 返回 `(nextOpcodeId, 剩余状态...)`，首返回值即下一状态 ID
- 状态元组里**混入函数引用**（如 `..., b[47], b[66], ...`）——
  原语指针也走状态传递，进一步隐藏「哪个函数是解密器」
- 效果：不存在任何单个巨型分发函数，opcode→动作映射散落在 ~50 个
  第二层 + 142 个第三层函数里，静态分析必须跨函数重建全图

---

## 3. blob 解码流水线（Stage 1）

```
RC（74.5KB 长字符串，前缀 "=[LP" 4 字节标记）
  → string.sub(RC, 5)                    剥掉 4 字节标记
  → string.gsub(s, XC, pC)               字符级转义（见下）
  → buffer.fromstring(text)              文本 → 二进制 buffer（z）
  → 状态机 handler 组逐字节解码：
      - 5 字符 token 序列 → 还原为对应特殊字符
      - 其余字符 = base-N 数据（字符类 [ -'}~]，ASCII 32..126 中 10 个特殊字符被 token 化）
      - 数值重建：7-bit 分块（见 §4.2）
      - 字节写入：b[47](buf, pos, b[8](K1, buf2[pos], K2))
        b[8] = bit32.bxor —— XOR 流密码，密钥流 = 位置相关 (const + pos) % 256
  → 还原出：指令流 + 常量池 + 原型树（写入多个 buffer）
```

**转义表 pC（10 个特殊字符 → 5 字符安全 token）**：

| 特殊字符 | token |
|---|---|
| `"` | `=W8+C` |
| `'` | `*cZ7D` |
| `%` | `]JEL>` |
| 空格 | `{5Cw,` |
| `$` | `--vv@` |
| `!` | `2[,6K` |
| `~` | `ix|L9` |
| `#` | `3;MLo` |
| `}` | `,;^6{` |
| `&` | `kTXL7` |

**XOR 密钥流**（handler `kC` 等中可见）：
```lua
local E = (213*M + 225) % 256                 -- 密钥派生（每构建不同常数）
b[47](F, H, b[8](160, b[66](K, H+q), E))      -- writeu8(buf, pos, bit32.bxor(160, src[pos], key))
```
常数（160/213/225 等）每次构建随机 → **密钥不可静态提取**。

**二级 blob**：另有 ~597B 与 ~313B 两段编码字符串（元数据/密钥相关），
由其他 handler 消费。

---

## 4. 执行 VM（Stage 2）

### 4.1 寄存器 VM + SoA + 二叉决策树

```lua
while true do
  local q = J[Q]            -- J = opcode 数组，Q = pc（SoA！）
  if q<39 then if q<19 then ...
  ...                       -- 二叉决策树派发
  Q = n[y[Q]]               -- 间接跳转（目标在寄存器里）
  n[y[Q]] = m[Q] - n[R[Q]]  -- 寄存器赋值：dst[y[Q]] = m[Q] - src[R[Q]]
```
- SoA 数组：`J`(opcode) / `m`(常量/立即数) / `R`(寄存器 A) / `y`(寄存器 B/目标) / `t`(辅)
- 寄存器文件 `n` 由数组索引寻址（`n[y[Q]]`）
- **间接跳转** `Q = n[y[Q]]`：跳转目标可以是寄存器值
- **超级指令**：单个 handler 融合「算术 + 比较 + 条件分支」
  （如 handler `y`：`w = b+z; 比较链; K[5]=w; 分支到 151/145`——
  加减法与比较在一条 handler 内完成，操作数编码在数据里而非指令里）

### 4.2 7-bit 分块寄存器/操作数编码（v15 核心变化之二）

```lua
local F, H, E = b[66](w, 2+u), Z-128, 128*(M-128)      -- 读 7-bit 分块
local z = 16384*G + (F + H + E)                          -- 128 进制重建
local b = K - 128; local u, z = F*128 + b, 2 + Z;        -- 另一种组合
return 87, z - 4294967296, Z                              -- 2^32 归一化（无符号→有符号）
```
- 每个「逻辑寄存器值」被**拆成多个 7-bit 块**存放在不同状态槽，
  使用时按 128 进制（128/16384=128²/2097152=128⁵）重建
- 操作数宽度**自动选择**：`H = F<16384 and 7 or (F<2097152 and 14 or 21)`
  ——按数值量级选 7/14/21-bit 编码（变长！）
- 配合 `bit32.lshift/rshift/countrz/bnot/and/or` 完成位重建
- 效果：反编译器无法把「一段连续内存 = 一个寄存器值」，
  单点 patch/断点无法定位逻辑寄存器

### 4.3 帧模型：每帧一个 Lua 闭包（continuation-passing 风格）

```lua
while true do
  if l<=0 then return A
  else k=u[10]; m=u[16]; ... l, A = 0, function(...) local k,s,W,p,L,C,I,h; ...
```
- 每次「函数调用」→ VM **返回一个新的 Lua 闭包**（新帧，带自己的局部
  寄存器数组 `k/s/W/p/L/C/I/h` 与状态表 `u[1..16]`），由上层继续调用
- 帧内寄存器数组通过 `b[22]` = `table.create(size)` 预分配
- 效果：每个帧是独立的 Lua 栈帧（对 hook/反调试友好），
  调用关系不在单一循环里可见

### 4.4 协程驱动

- `b[38]` = `coroutine.yield`：handler `UL` 中 `local o = b[38]; o()`
  ——VM 用**宿主协程**承载用户代码（别名表含 create/resume/wrap/
  running/status/yield/close/isyieldable 全套 8 个）
- 用户程序的 `coroutine.*` 映射到宿主协原语；跨协程多值走宿主协议

### 4.5 环境 / 错误

- `[25]=setfenv` `[34]=getfenv`：环境重建（全局访问/闭包环境）
- `[14]=xpcall` `[81]=pcall` `[16]=error`：错误包装
- 正则 `:(%d+)[:\13\10]`：**解析 Luau 错误信息的行号并重写**
  ——报错信息混淆（防从报错泄漏源码结构/行号）
- `__index`、`string` 字面量：环境表/元表重建用

---

## 5. 原语别名表（数字槽 → 真实函数，共 ~60 个）

| 槽 | 函数 | 槽 | 函数 |
|---|---|---|---|
| 1 | buffer.fill | 47 | buffer.writeu8 |
| 7 | string.unpack | 51 | buffer.create |
| 8 | **bit32.bxor（密码原语）** | 55 | buffer.tostring |
| 10 | bit32.rshift | 56 | buffer.copy |
| 13 | table.pack | 57 | buffer.writeu32 |
| 14 | xpcall | 58 | typeof |
| 15 | next | 64 | rawset |
| 16 | error | 66 | buffer.readu8 |
| 17 | Vector3.new | 67 | string.match |
| 20 | bit32.band | 68 | buffer.readi16 |
| 21 | coroutine.resume | 69 | buffer.readu16 |
| 22 | table.create | 70 | string.find |
| 23 | bit32.countrz | 71 | bit32.bnot |
| 24 | string.rep | 72 | string.pack |
| 25 | setfenv | 75 | rawget |
| 28 | coroutine.running | 76 | assert |
| 31 | coroutine.wrap | 77 | coroutine.close |
| 34 | getfenv | 81 | pcall |
| 35 | coroutine.status | 83 | buffer.writei |
| 36 | buffer.len | 84 | type |
| 37 | tonumber | 90 | table.insert |
| 38 | coroutine.yield | 91 | buffer.readf32 |
| 39 | table.concat | 93 | string.sub |
| 41 | string.gmatch | 96 | **LCG PRNG（内联函数）** |
| 42 | buffer.readstring | 97 | tostring |
| 43 | getmetatable | 99 | bit32.lshift |
| 46 | buffer.readi32 | 102 | table.move |
| 53 | select | 104 | string.byte |
| — | — | 107 | buffer.fromstring |
| — | — | 108 | buffer.readu32 |
| — | — | 109 | string.char |
| — | — | 112 | coroutine.create |
| — | — | 113 | coroutine.isyieldable |
| — | — | 114 | unpack |
| — | — | 116 | string.format |
| — | — | 118 | string.gsub |
| — | — | 122 | bit32.bor |
| — | — | 124 | setmetatable |
| — | — | 125 | buffer.readf64 |
| — | — | 126 | vector.create |

（数字槽本身每次构建也随机分配）

---

## 6. PRNG：状态机化的 LCG（密钥源头）

```lua
[96] = function(...) return function()
  local u = 5
  while true do
    if u<=3 then if u<=1 then if u<=0 then
      b[1][4][b[1][7]] = (985577*x + 234540221) % 268435456   -- 4 步 LCG
      b[1][4][b[1][7]] = (639055 *x + 45488747)  % 268435456
      b[1][4][b[1][7]] = (627209 *x + 257144031) % 268435456
      b[1][4][b[1][7]], u = (1031513*x + 219713329) % 268435456, 6
    ...
```
- **LCG mod 2^28**（268435456 = 2²⁸），乘数/增量每组 4 步各不相同
  （985577/639055/627209/1031513/458407/… 全为每构建随机值）
- 整个 PRNG 被**展开成状态机**（u=0/1/2/3/4/5/6 状态），
  状态存于 VM 帧寄存器 `b[1][4][b[1][7]]`
- 输出驱动 XOR 密钥流 → **密钥完全在运行时生成，静态不可见**

---

## 7. 与 v14 的对比（版本演进）

| 维度 | v14 | v15 |
|---|---|---|
| 字节码格式 | SoA（e/u/Y/L/H 数组） | SoA（J/m/R/t/y 数组）+ **buffer 二进制存储** |
| 分发 | 单层二叉决策树 | **三层分发**（顶层状态机 → 子分发器 → 142 handler） |
| 状态传递 | — | **位置参数元组**（混入函数引用） |
| 寄存器 | 常规 | **7-bit 分块编码 + 变长 7/14/21-bit + 2³² 归一化** |
| 帧模型 | 循环内执行 | **每帧新 Lua 闭包（CPS 风格）+ 宿主协程驱动** |
| 字符串 | LZW+Base36 解码器 | **base-N（95 字符类）+ 10 字符 token 转义（pC）** |
| 密码 | XOR（bit 操作） | **bit32.bxor 流密码 + 位置相关密钥 (const+pos)%256** |
| 密钥来源 | 嵌入 | **状态机化 LCG PRNG（mod 2²⁸，每构建常数）** |
| 指令 | 常规 | **超级指令**（算术+比较+分支融合，操作数即数据） |
| 数据载体 | table/string | **Roblox buffer（不透明二进制内存）** |
| 目标 | Roblox | Roblox（更深度绑定：buffer/Vector3/typeof） |
| 反篡改 | 完整性校验 | 样本中未见 os.clock/校验和（可能弱化或内嵌于 handler） |
| 错误处理 | — | **报错行号解析重写**（`:(%d+)[:\r\n]`） |

---

## 8. 对本项目的启示（→ 更新 VM 设计）

1. **数据层**：v15 用 Roblox buffer 是「Roblox 专属」换「不透明内存」；
   **我们双目标（5.1+Luau）必须用纯 Lua 载体（table/string）**——
   这是我们的兼容性优势，但要在编码上补偿不透明度损失
   （建议：字符串载体 + 7-bit 分块 + token 转义，等效不透明）
2. **7-bit 分块寄存器编码**：✅ 采纳（成本：解释器体积/速度；
   收益：彻底破坏寄存器模式匹配）——作为 VM 预设的可选档
3. **三层分发 + 状态元组**：✅ 采纳（比单层决策树抗分析能力强得多；
   Rust 侧生成器按随机树深度 2~4 层生成）
4. **LCG PRNG 状态机化**：✅ 采纳（mod 2²⁸/2³¹-1 均可，
   常数每构建随机，PRNG 本身展开成状态机发射）
5. **XOR 流密码 + 位置密钥**：✅ 采纳（bit32 在 Luau 可用；
   5.1 目标回退到 256×256 查找表 XOR——双方言同一套密钥派生）
6. **每帧 Lua 闭包（CPS）**：⚠️ 评估（对 Roblox 栈限制/hook 有优势，
   但性能损失大；v1 先用单循环 + 可选「帧闭包」高级档）
7. **超级指令**：⚠️ 评估（编码复杂度高；v2 再做）
8. **报错行号重写**：✅ 低成本高收益，采纳
9. **反篡改**：v15 样本未见强反篡改 → **我们的校验和 + 时间陷阱
   作为差异化卖点保留**（研究笔记 L7 不变）
10. **每构建唯一性**：v15 的随机面 = 数字槽分配 + 常数 + 字符 token +
    派发树 + 分块方式 → 我们的 VMC 随机面对齐这些维度

### 脱壳可行性评估（该样本）

- 静态：需重建 142 handler 的调用图 + 复现 LCG/密钥流/7-bit 解码 +
  base-N 还原 → 工作量大但可行（每版本需重做，无通用工具）
- 动态：hook 第 2 层子分发器的入口（状态 ID 收敛点）记录
  (状态, buffer 变化) 序列，再离线重建语义 → 仍是当前最快路径
- 结论：v15 的防护强度主要在「每版本重做」+「buffer 不透明性」，
  动态 hook 并未被完全封死（无 os.clock 强时间陷阱是样本弱点）

---

## 9. 样本关键位置索引（便于复查）

| 内容 | 文件偏移 |
|---|---|
| 模块表开始（jC=…） | 0（第 2 行起） |
| `pC` 转义表 | ~169,500 |
| `FC` 引导状态机 | 157,123 |
| `L` 第二状态机 | ~158,200 |
| `RC` blob 定义 | 75,416 |
| blob 解码入口（gsub+fromstring） | ~163,658 |
| LCG PRNG（`[96]`） | ~56,400 |
| `XC` 字符类 | ~53,260 |
| 报错行号正则 | ~55,298 |
| 入口 `:FC()(...)` | 文件末尾 |
