# v15 结构 100% 还原计划

> 样本：`origin/main:luraph15.txt` ≡ `samples/luraph15.txt`（171 753 字节，已逐字节核对）
> 头部：`-- This file was protected using Luraph Obfuscator v15.0 [https://lura.ph/]`
> 对照输出：本仓库 `--preset vm`（`luraph-rs/examples/basics.vm.5.1.lua`）
> 日期：2026-08-25
> 目的：让**我们发出的脚本在结构指纹上与该 v15 样本同族**，而不是逐字节复制这一份（v15 每构建都换槽号/常数/树形，逐字节克隆既不可能也没有产品意义）。

---

## 0. 「一模一样 / 100% 还原」的可验收定义

**不是**：输出与 `luraph15.txt` 逐字节相同。

**是**：一份结构指纹清单，人工或脚本对照样本，**每一条都成立**。对照物是「我们新发的 `--preset v15` 产物」vs「这份 v15 样本的形态」，不是语义实现细节的内部命名。

### 0.1 结构指纹（验收清单，必须全部打勾才算 100%）

| # | 指纹 | 样本实测 | 我方 `--preset vm` 现状 |
|---|---|---|---|
| F1 | 文件 2 行：注释头 + 一条 `return` | `2` 行；头注释 + `return setmetatable(...)` | 2 行，但是 `local k=function()...` + L5 `loadstring` 容器，**无** `setmetatable` 入口 |
| F2 | 入口形态 | `return setmetatable({...}, {}):FC()(...)` | `loadstring(DEC(...))()` 包一层解释器 |
| F3 | 整个程序 = **一张模块表** | 具名 handler + `[N]=原语` + 命名字段混存 | 一个 `local VM = function`，原语在函数内 `P[n]=` |
| F4 | handler 数量级 | `=function(` **146**；具名函数 **141** 个不重复 | 整文件 `function(` **2** 个（解释器 + 一两个闭包） |
| F5 | 三层分发 | 顶层 `FC`/`L` while 状态机 → 子分发器（`iL` 330 次 / `vL` 239 次）→ handler `return <id>, ...`（**351** 次，**136** 个不同状态 ID） | **单层** `while true` + 二叉/平铺 `if oc==` 决策树，handler 不返回下一状态 |
| F6 | 状态 = 位置参数元组 | `function(b,u,z,Z,o,w,K,G,...)`；`return 93,w,b[1],b[2],...` | 局部变量 `oc,a,b,c,d,pc`，不跨函数传状态 |
| F7 | `continue` 扁平叶 | **45** 处 `... = b:X(...); continue` | **0**（5.1 目标禁止 continue） |
| F8 | 长串 blob + pC + XC | `RC=[=[LPH:...]=]` **74 572** 字节；`pC` 10 特殊→5 字符 token；`XC="[ -'}~]"` | 载体在 L5 密文短串里，**看不到** `RC`/`pC`/`XC`/`[=[LPH:` |
| F9 | 数字槽原语（模块表字段） | `[51]=buffer.create` `[58]=typeof` `[104]=string.byte` 等，槽 0..126，约 **81** 个独立槽 | 15 个原语在解释器**内部**表，无 buffer/bit32/typeof |
| F10 | 裸 API 名极少 | 全文件仅 **29–30** 个 `"..."` 字面量；`buffer.` 17 / `bit32.` 7 / `string.` 11 次且几乎都在模块表初始化 | L5 之后可见的是解密器/金丝雀，**不是**模块表形态；未混淆模板里 `string.byte` 成片出现 |
| F11 | 四段独立分发循环 | 动态：site2 构建 / site3 校验 / site5 解码 / site4 执行 + 1 条死路径 | **一个** `run` 循环既 fetch 又执行 |
| F12 | CPS 帧 | 调用返回新 Lua 闭包；普通帧 `[73]` + 协程帧 + `UL` 跨 yield 传表 | `makefn` 一次造闭包，之后同一 `run` 循环解释，**不是**每帧新闭包 |
| F13 | 7-bit 逻辑寄存器拆块 | 上下文表 K 多槽重建；`z-4294967296`；`bit32.*` 位重建 | 操作数 varint 已有 7/14/21；**寄存器文件仍是一张 V[]**，无拆块、无 bit32 |
| F14 | SoA 执行期数组 | `J[Q]`/`R[Q]`/`y[Q]`/`m[Q]` 分数组；pc 可 +1/+2 | parse 后有 `W/SA/SB/SC/SD`，但藏在函数局部，不是模块表字段 |
| F15 | 状态机化 LCG | `[96]` 4 步 LCG mod 2²⁸ 展开成 while；另有 `[63]` 2 步实例 | 密钥是构建期嵌入的 24B 加性流，**输出里没有** LCG 状态机 |
| F16 | XOR 流 + 位置密钥 | `bit32.bxor` + `(const+pos)%256` 写 `buffer` | `add8` 算术密码，无 XOR、无 buffer 写回 |
| F17 | 错误重写状态机 `nL` | 模块表字段，正则 `:(%d+)[:\r\n]`，level 0/1 重抛 | L7 有行号重映射，但是 L5 容器层，不是模块表 handler |
| F18 | 无 `os.clock` / 无外层校验和 | 样本 **0** 次 `os.clock` | L7 **故意**加时间陷阱 + 校验和（差异化，和样本不像） |
| F19 | Roblox 绑定 | `buffer.*` / `typeof` / `Vector3` / `vector.create` / `setfenv` | 双目标纯 Lua，**拒绝**这些才能跑 5.1 |
| F20 | while 形态 | **14** 个 while（`true` / `u` / `o` / `Z` / `F`），不是一个大循环 | 容器 + 解释器里少量 `while true`（含时间陷阱死循环） |

按上表：**20 条指纹里我方目前大约 2–3 条部分沾边**（minify 单行、有私有 VM、操作数 7-bit、有 token 思想），**外观 0 条对齐**。这不是「再加几个 M5 开关」，是换一代发射器。

---

## 1. 样本再读结论（相对 `docs/luraph15-analysis.md` 的增量）

本次直接从 `origin/main:luraph15.txt` 抽的硬数字：

```
return setmetatable({ jC=function(...) ... end, ..., [51]=buffer.create }, {}):FC()(...)
```

- 入口 **不是** `loadstring`。整个脚本的值就是模块表，`:FC()` 跑引导状态机，最后返回用户入口再 `(...)`。
- 模块表同时是：opcode handler 仓库、原语别名表、常量箱（`QL=false`、`AL="n"`）、blob（`RC`）、转义表（`pC`）、字符类（`XC`）。
- 分发是 **CPS + 状态 ID**：handler 几乎都以 `return <nextId>, <元组...>` 结尾（351 次 / 136 个 ID）。静态看不到一张 opcode switch。
- 子分发器高度收敛：方法调用 `b:iL(...)` 330 次、`b:vL(...)` 239 次——这两处就是动态 hook 的收敛点。
- 数据层是 **Roblox buffer + bit32**，不是 string。`buffer.create` 是表的最后一个字段，和 handler 写在同一张表里。
- `continue` 是 Luau 形态的一等公民（45 处），用来把决策树压成扁平叶。
- **没有** `os.clock`。我们的 L7 时间陷阱会立刻让产物「看起来不像 v15」。

分析文档 §8 当时的决策（拒 buffer、拒 Vector3、CPS/超级指令延期、保留校验和/时间陷阱）在 **双目标产品** 上仍然正确，但和「看起来就是这份 v15」**直接冲突**。

---

## 2. 必须先拍板的分叉

100% 结构还原 **不能** 同时满足「输出长得像这份 Roblox v15」和「同一份输出在 Lua 5.1 上可跑」。

| 路线 | 目标环境 | 和样本的像 | 5.1 | 建议 |
|---|---|---|---|---|
| **A. `--preset v15`（Roblox 克隆档）** | Luau + `buffer`/`bit32`/`typeof`/`setfenv` | 可以冲 F1–F20 | 放弃（或另出 polyfill 档，但指纹会脏） | **要 100% 外观就走这条** |
| **B. 双目标结构孪生** | 5.1 + Luau，table/string 代替 buffer | F1–F8、F11–F12、F15 可像；F9/F13/F16/F19 永远不像 | 保住 | 这是旧设计；上限大约 70% 指纹 |
| **C. 维持现状 + 加深随机面** | 现状 | 外观仍是「L5 容器包解释器」 | 保住 | 达不到本题 |

**本计划按路线 A 写。** 现有 `--preset vm` 继续服务 5.1+Luau，不拆。v15 档是第三条发射管线。

若你改口要路线 B，把第 4 节里所有 `buffer`/`bit32`/`continue`/`typeof` 换成 table/string + 嵌套树即可，其余骨架共用。

---

## 3. 目标产物形态（发射器要吐出来的东西）

```
-- This file was protected using luraph v<ver>   -- 可关；默认模仿样本头
return setmetatable({
    -- ① ~40–160 个具名 handler：每个吃 (self, 状态元组...)，return nextId, ...
    -- ② 第二层子分发器：while + 二叉/continue 叶，按 ID 调 ①
    -- ③ 顶层 FC / L / _L / M：引导、解码、执行 三段状态机
    -- ④ [随机槽] = buffer.* / bit32.* / string.* / table.* / coroutine.* / typeof / setfenv ...
    -- ⑤ 命名字段混存：RC（长串 blob，前缀 LPH:）、pC、XC、nL、UL、QL、AL、...
    -- ⑥ 帧运行器 [rN] / [rC]：入场解包 20+ 原语局部；普通帧 + 协程帧
}, {}):FC()(...)
```

语义上仍跑**我们自己的 ISA**（不必复刻样本那 136 个状态 ID 的精确含义——那些是**这一次构建**的随机面）。结构上必须让「表 + 三层分发 + blob + 数字槽原语 + CPS 帧」齐套。

---

## 4. 分阶段实施（建议顺序，每阶段有独立可勾验收）

### P0 — 结构契约与对照脚手架（约 1 天）

1. 把 §0.1 做成 `luraph-rs/tests/v15_fingerprint.py`（或 shell）：对一份输出数 `=function(`、`continue`、`setmetatable(`、`return setmetatable`、`RC=`/`pC=`/`XC=`、`buffer.`、`return %d,`、while 个数、字面量个数。
2. 样本跑一遍得到**基线黄金数**（已写在 §0.1 / §1）。
3. 我方当前 `vm` 产物必须 **fail** 这份脚本（防止假通过）。
4. 新增方言/预设：`--preset v15` + `--dialect luau`（硬性）；5.1 直接报错说明该档需要 buffer。

**验收**：脚本对样本全绿、对现 `vm` 全红；CLI 开关存在。

### P1 — 外壳换代：模块表 + `setmetatable():FC()()`（约 2–3 天）

把 `template.rs` 的「一个 `local VM=function` + 事后 L5 包一层」改成：

- 生成一张表字面量（handler / 槽 / 字段）。
- **v15 档关闭 L5 `body` 和 L7 时间陷阱**（否则 F2/F18 永远失败）。校验和若要留，必须藏进某个 handler，不能是外层 `os.clock`。
- minify 仍开，保证 2 行。
- 入口固定形态：`return setmetatable(T, {}):<引导名>()(...)`，引导名每构建随机（样本是 `FC`）。

此阶段 handler 可以先是空壳 / 单循环仍藏在一个「执行」handler 里——先把 **F1 F2 F3 F10 的外壳** 做对。

**验收**：指纹 F1–F3、F10（字面量数进入几十档）、F18（无 `os.clock`）通过；`print(1+2)` 在 Luau CLI 上仍能跑对（内部可暂用旧 run 循环）。

### P2 — 数字槽原语 + 入场解包 + 模块表混存（约 1–2 天）

- 原语集扩到样本同款量级（buffer / bit32 / string / table / coroutine / typeof / setfenv / getfenv / pcall / xpcall / next / select / unpack …）。槽号 Fisher-Yates。
- 帧运行器头部一次性 `local a,b,c,... = self[17], self[8], ...`（P 已有雏形，要升到**模块表字段**而不是函数内 `P`）。
- 常量、正则、`false`、`"n"` 混进同一张表。

**验收**：F9；输出里 `buffer.create`/`bit32.bxor` 只出现在表初始化右侧。

### P3 — 三层分发 + 状态元组 + continue 叶（约 3–4 天，最重）

这是和样本「像不像」的核心。

1. 把 41 条 ISA 拆成 **细粒度 handler**（每函数 1–4 个状态 ID，结束 `return next, ...`）。
2. 生成 8–20 个第二层分发器：`while <flag> do if id<=T then ... else ... end`，叶 = `id,... = self:hX(...); continue`。
3. 顶层 `FC`（引导+解码）和 `L`（执行）两个状态机，初始器 `_L`/`M` 返回 `(true, startId, nil×N)`。
4. 状态元组长度 8–16，**混入函数引用**（原语指针走参数，不走全局名）。
5. 每构建：handler 名、分发器切分、阈值、元组排列、枢纽 ID 全随机。
6. 保留 1 条永不命中的死分发循环（样本 site1）。

**验收**：F5 F6 F7 F11；`return <数字>,` 次数与不同 ID 数进入样本同数量级（≥100 次 / ≥30 个 ID，不必等于 351/136）。

### P4 — blob 流水线改成「看得见的 v15」（约 2 天）

现有 `Carrier`（base-94 + 保留前缀 token）语义可用，但**发射形态**必须改：

- 主程序字节码进 **一张长字符串** `RC=[=[LPH:...]=]`（或 `[==[` 避冲突），不是 L5 的 3–5 段短串。
- 表上挂 `pC`、`XC`；引导阶段 `string.sub` + `gsub(XC,pC)` + `buffer.fromstring`。
- XOR 流：`bit32.bxor` + 位置密钥；密钥来自 **输出里的 LCG 状态机**（P5），不是构建期写死的 24B。
- 解码写进 `buffer`，再拆成 SoA 平行数组（可仍是我们的 ISA）。

**验收**：F8 F16；打开产物能用眼睛看到 `RC=[=[LPH:` 和 `pC={`。

### P5 — 状态机化 LCG（双实例）（约 1 天）

- 解码阶段、执行阶段各一个 LCG，mod 2²⁸，乘数/增量每构建随机。
- **PRNG 本身展开成 while 状态机**，写在模块表的某个数字槽（样本 `[96]`/`[63]`）。
- 输出里禁止出现连续 24 字节密钥字面量。

**验收**：F15；对产物 grep 不到长密钥，能 grep 到 `% 268435456` 或等价大常数。

### P6 — CPS 帧 + 双运行器 + UL（约 3 天，语义雷区）

- 用户函数调用 = 返回新闭包（新寄存器银行 + 新状态表），上层再调。
- `[普通帧]` / `[协程帧]` 两套；`UL`：先空 `yield`，再 `yield(true,k,v)` 传表。
- **upvalue 单 cell 模型必须在新帧模型里重做**（`docs/vm-l6-implementation.md` §8.1）。这是最容易把 204 矩阵打爆的一步。
- 协程：用户 `coroutine.*` 映射到宿主；跨 yield 多值走 UL。

**验收**：F12；`tests/cases/stress_upvalues.lua` + `stress_coroutines.lua` 在 `--preset v15` 下与原脚本 stdout/exit 一致。

### P7 — 7-bit 寄存器拆块 + 四银行 + 超级指令（约 2–3 天）

- 逻辑寄存器 = 上下文表里 2–3 个 7-bit 槽，读写都重建；槽位每构建随机。
- 四个 SoA 银行表共享一个 metatable（样本 10.5）。
- 超级指令：选 8–15 条高频「算术+比较+分支」融合 handler（样本 handler `y` 形态），降低「一条 ISA = 一个 handler」的规律。

**验收**：F13 F14；反汇编/读源看不到 `V[a+1] = V[b+1] + V[c+1]` 这种直给式。

### P8 — `nL` 错误重写进模块表 + 指纹全绿 + 语料（约 1–2 天）

- 把现 L7 行号重写挪成模块表状态机（正则拆开拼接，level 0/1）。
- **不要**外层 `os.clock` 死循环（若仍要反调试，做成 handler 内的隐性计数，样本没有这项）。
- `tests/run_presets.sh` 增加 `v15`（仅 luau）；官方矩阵的 `vm`/`high` **不准回归**。
- 人工：`luau` 跑通 `basics`/`functions`/`game_loop`/`stress_*`；对产物跑指纹脚本 20/20。

**验收**：F17 + 全表 F1–F20；v15 档语料绿；旧预设仍 204+405。

---

## 5. 明确不做 / 以后再说

| 项 | 原因 |
|---|---|
| 复刻该样本的 **136 个状态 ID 语义** | 那是一次构建的随机面，不是 v15 规范 |
| 复刻 Roblox `Vector3`/`vector.create` 序列化 | 用户程序值透传即可；样本有是因为游戏数据 |
| 字节级兼容 Luraph 官方解码器 | 我们 ISA 自定；目标是**结构同族**，不是能被他们的 VM 执行 |
| 让 `--preset v15` 跑在 Lua 5.1 | 与 F9/F13/F16/F19 冲突 |
| 外层 L5+L7 时间陷阱与 v15 档同时开 | 会毁 F2/F18 |

---

## 6. 工程落点（改哪些文件）

| 文件 | 角色 |
|---|---|
| `luraph-rs/src/vmgen/template.rs` | **重写发射形态**（模块表 / 三层分发 / 帧运行器） |
| `luraph-rs/src/vmgen/isa.rs` | 状态 ID 分配、SoA 仍用；加 LCG 常数、枢纽 ID |
| `luraph-rs/src/vmgen/compiler.rs` | 跳转改为「状态 ID」而非 pc；闭包改为「造帧描述符」 |
| `luraph-rs/src/main.rs` | `--preset v15`：关 body/antidbg 时钟、强制 luau、走新模板 |
| `luraph-rs/src/body.rs` / `antidbg.rs` | v15 档跳过或改内嵌 |
| `luraph-rs/tests/v15_fingerprint.py` | 结构指纹 |
| `luraph-rs/tests/run_presets.sh` | 加入 v15（luau only） |
| `docs/luraph15-analysis.md` | 每阶段把「我方状态」从设计改成 ✅ 实现 |

旧 `vm` 模板不要删，用 `generate_legacy` / `generate_v15` 分叉，避免 5.1 矩阵陪葬。

---

## 7. 工作量与风险

| 阶段 | 估时 | 主要风险 |
|---|---|---|
| P0 脚手架 | 1 天 | 指纹写松会假绿 |
| P1 外壳 | 2–3 天 | 关 L5 后字符串/体积形态大变，minify/mangle 交互 |
| P2 槽/解包 | 1–2 天 | 低 |
| P3 三层分发 | 3–4 天 | 状态元组排列与 handler 签名必须同一套 RNG |
| P4 blob 形态 | 2 天 | 长串 `]=]` 转义；buffer 边界 |
| P5 LCG | 1 天 | 5.1 没有这一档，Luau `bit32` 即可 |
| P6 CPS 帧 | 3 天 | **upvalue / 协程 / 尾调用**，矩阵最容易红 |
| P7 拆块+超指令 | 2–3 天 | 性能再降一档（样本约 190 万 op/s，可作上限参考） |
| P8 收口 | 1–2 天 | 指纹与语料对打 |

合计大约 **2.5–4 周** 单人（含回归），其中 P3+P6 占一半。

**最大风险**：CPS 帧 × 单 cell upvalue。v15 用「每帧新闭包 + 活引用」绕开了一部分问题；我们 §8.1 的三种描述符要映射到「父帧 cell 对象作为 upvalue 传进子帧」，不能再 GetUp 拷贝。

---

## 8. 建议的开工口令

1. 先确认走 **路线 A**（本计划）还是 B（双目标孪生、放弃 buffer 指纹）。
2. 确认后从 **P0 + P1** 开始：先让 `print(1+2)` 的产物 **长得像** `return setmetatable({...},{}):FC()(...)`，再往里填三层分发。
3. 不要一上来改 ISA 语义。外壳不像，后面全白做。

未确认路线之前，不改 `vmgen/` 发射逻辑。
