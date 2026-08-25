# Lua 混淆技术研究笔记（学习记录）

> 记录人：Agent Mode ｜ 日期：2026-08-23（2026-08-25 补 §3 两行方言差异 +
> §5 实现现状 + §6 语料落地状态）
> 用途：商业级 Lua 混淆器（对标 Luraph）设计依据。所有「实测」条目均用
> Lua 5.1.5 与 Luau 0.735 真实解释器验证过，不是纸面推断。

---

## 0. 目标定义

| 项 | 内容 |
|---|---|
| 对标 | Luraph（2017 年至今，Roblox 生态公认最强的商业混淆器） |
| 目标方言 | Lua 5.1 与 Luau 双目标 |
| 混淆器实现语言 | Rust（std-only，零第三方依赖） |
| 产物形态 | 命令行工具 → 输出「受保护 Lua 源码」（内含自定义 VM + 加密字节码 + 运行时加载器） |
| 商业级定义 | 多层纵深防御 + 每构建唯一 VM + 标准反编译器完全失效 + 反篡改 |

---

## 1. Luraph 逆向研究结论（来自公开逆向分析）

### 1.1 总体架构

Luraph 的核心是**字节码虚拟化（VM 虚拟化）**：

```
源码 → (Luraph 编译器) → Luraph 自定义指令 IR（vm_instructions 表：
        每条含 opcode / 寄存器 / 常量）→ 嵌入脚本的「Lua 解释器」
        （LuraphInterpreter / InterpretFunc，带虚拟指令指针）顺序执行
```

- 标准 Lua 反编译器只认原生 5.1/5.4 字节码格式，对 Luraph 指令**完全无效**
- 每个脚本生成**唯一的 VM**（指令编码/结构每次构建都不同）→ 通用脱壳工具一次只能对付一个版本
- 多层如洋葱：攻破一层还有下一层

### 1.2 Luraph v14 的具体实现细节（逆向者披露）

1. **SoA 字节码格式**（Structure of Arrays）：指令拆成多个平行数组
   `e[pc]=opcode, u[pc]=A, Y[pc]=B, L[pc]=C, H[pc]=const`，
   而非标准 Lua 的 AoS（`opcode | A<<6 | B<<14 | C<<23`）→ 格式不可识别
2. **二分决策树派发**：主循环不用 switch/表查找，而是嵌套 if-else
   二叉树（`if not(d < 55) then ...`）逐级匹配 opcode → 难以定位 handler
3. **Unrolled VM**：把指令流展开成巨量顺序 Lua 语句，替代传统分派循环
   （兼顾性能与混淆，也导致脱壳必须「re-roll」）
4. **字符串/常量/字节码统一走自定义解码器**（LZW 风格 + Base36），
   运行时动态重建 → 静态看不到任何明文常量
5. **混合进制数字字面量**：十六进制/二进制/下划线分隔混用，防模式搜索
6. **死代码注入**：数据表里塞冗余/nil 填充项，helper 函数里放假逻辑
7. **反篡改**：字节码变异（随机化无用信息）、完整性校验

### 1.3 行业对比（摘要）

| 混淆器 | 强度 | 特点 |
|---|---|---|
| Luraph | 极高 | 自定义 VM 最复杂、频繁更新、VM 完整性检查强 |
| Ironbrew v2/v3 | 高 | 自定义字节码，复杂度低于 Luraph |
| Xen (Synapse) | 高 | 字节码加密强 |
| LVM | 中高 | 字节码格式相对简单 |

### 1.4 Luraph v15 实测样本分析（用户提供样本）

> 完整报告：`docs/luraph15-analysis.md`（含样本位置索引/别名表/脱壳评估）

v15.0 样本（171KB，Roblox 目标）相对 v14 的关键演进：

1. **数据层换成 Roblox `buffer` 二进制内存**（不透明 + 无类型），
   原语经数字槽别名（buffer.readu8/bit32.bxor/coroutine.yield… ~60 个）
2. **三层分发**：顶层 while 状态机 → 子分发器（~50 个）→ 142 个细粒度 handler，
   状态以位置参数元组传递（混入函数引用）
3. **7-bit 分块寄存器/操作数编码**：逻辑值拆成 7-bit 块按 128 进制重建，
   变长 7/14/21-bit，2³² 归一化 → 寄存器模式匹配彻底失效
4. **密钥 = 状态机化 LCG PRNG**（mod 2²⁸，乘数/增量每构建随机，
   PRNG 本身展开成状态机）驱动 **bit32.bxor 位置密钥流**
5. **base-N 编码**（字符类 ASCII 32..126）+ 10 特殊字符 → 5 字符 token 转义
6. **每帧一个新 Lua 闭包**（CPS 帧模型）+ 宿主协程驱动用户代码
7. **超级指令**（算术+比较+分支融合在单个 handler）
8. 报错行号解析重写（`:(%d+)[:\r\n]`）
9. 弱点：样本中未见 os.clock 时间陷阱/显式校验和 → 动态 hook 仍可行

对本项目设计的采纳决策见 `docs/luraph15-analysis.md` §8
（采纳：7-bit 分块/三层分发/LCG 密钥/token 转义/报错重写/VMC 随机面；
差异点：我们用纯 Lua table/string 数据层保 5.1+Luau 双目标 +
保留校验和/时间陷阱作为差异化）。

---

## 2. 分层混淆体系（本项目设计，L1–L7）

| 层 | 名称 | 技术 | 抗静态分析 | 性能代价 | 难度 |
|---|---|---|---|---|---|
| L1 | 词法层 | 名称混淆（局部/参数/upvalue）、minify | 低 | 无 | ★ |
| L2 | 字符串层 | 加密 + 运行时解密 + 字符串拆分 | 中 | 低 | ★★ |
| L3 | 控制流层 | 循环去糖、CFG 扁平化状态机、if 拆分、透明谓词、垃圾代码 | 高 | 中 | ★★★ |
| L4 | 数值层 | 整数拆分、安全浮点变换 | 低 | 无 | ★ |
| L5 | 整体加密层 | 全源码加密 + `loadstring` 运行时解密 | 中 | 低 | ★★ |
| L6 | **VM 层** | **自定义字节码 + 嵌入 Lua 解释器 + 每构建随机化** | **极高** | 高 | **★★★★★** |
| L7 | 反篡改层 | 校验和、时间陷阱、金丝雀 | 中 | 低 | ★★ |

**商业级 = L1+L2+L3+L5+L6+L7 全开**（L6 是护城河，其余层在 VM 之外再加纵深，
并作为「轻量预设」可单独出售）。

### 2.1 L1 词法层

- **名称混淆**：把每个局部变量/参数/upvalue/循环变量重命名为随机名
  - 关键：作用域安全。必须建**符号表**（scope stack + sym 对象），
    同名局部在不同作用域是不同变量；重命名集合要避开：关键字、
    程序用到的**全局名**（避免影子化）、其他符号的新名
  - `local function f() return f() end` 的自引用语义：
    Lua 中 f 在自己的函数体内**不可见**（解析器必须遵守，测试用例必须覆盖）
  - `self` 参数就是普通参数，可重命名（方法调用归一化为 `obj.method(obj, ...)`）
- 全局名不改（改全局需要整个 getfenv/setfenv 环境包装，5.1 可行但鲁棒性风险大，v1 不做）

### 2.2 L2 字符串层

- **密码方案（5.1 无位运算的约束下）**
  - `add8`（默认）：`c[i] = (s[i] + key[i%k] + i) mod 256`，
    解密 `d = c - key - i; r = d % 256; if r < 0 then r = r + 256 end`。
    **实测确认 5.1 与 Luau 的 `%` 都是 floor 语义 → 双方言共用同一解密代码**
  - `xor8`（可选）：5.1 无 XOR → 生成 256×256 查找表
    （`X[a]` 为 256 字节字符串，`X[a+1]:sub(b+1,b+1):byte()`），纯字符串运算，双方言通用
- **字符串拆分（unrolling）**：密文切成 1..N 段作多个字面量参数，运行时 `table.concat`
- **运行时解密加载器**：
  ```lua
  local function _dec(...)   -- 变参接收各分段
    -- select('#', ...) + table.concat + 逐字节解密
  end
  ```
  - 密钥碎片化嵌入（3–5 段字符串拼接后再 `string.byte` 展开为字节表），避免连续明文密钥
  - 解密函数本身的名字/内部变量同样被 L1 混淆
- 覆盖范围：所有字符串字面量（含转义序列、长字符串先归一化为带转义的短串字面量、
  垃圾代码里的字符串）

### 2.2.1 密钥暴露模型（密码学根本约束，2026-08-23 讨论确认）

**定理**：自包含脚本中，流密码密钥（或确定性生成密钥的种子/常数）**必然存在于
payload 中，且对持有脚本的攻击者 100% 可恢复**。
（解密必须能从 payload 自身复现密钥流 → 无外部秘密信道 → 密钥材料必在脚本内）

密钥存在的三种形态与暴露程度：

| 形态 | 暴露程度 |
|---|---|
| 明文连续字面量 | 最差（grep 即得，禁用） |
| 可见常数 + 运行时派生（Luraph LCG：乘数/增量/种子均为字面量） | **静态模拟 PRNG 即可离线算出全部密钥流** |
| 执行混淆代码派生（密钥 = VM 代码运行结果） | 静态难，动态执行一次即得 |

**混淆场景的致命便利**：攻击者拥有无限 (密文,明文) 对——明文就是用户程序本身，
跑一遍脚本即可观测；XOR 流密码下 `keystream = 密文 XOR 明文`，
**一对已知明文即可抽出密钥流，与所用流密码强度无关**（ChaCha8/AES-CTR 同）。

**结论（安全模型定调）**：
- 密钥可恢复是**设计前提**，不是缺陷——所有商业混淆器（含 Luraph）同模型
- 真正的壁垒只有两个：① 每构建唯一（密钥/常数/结构每次随机，通用脱壳无效）
  ② 提取成本（派生逻辑藏进混淆 VM，攻击者须模拟 VM 而非读字面量）
- ChaCha8 为减轮弱化版（8 轮），本场景下即使 ChaCha20 也无增益；
  **维持 LCG + XOR/add8 方案**，把预算投给随机面宽度（VMC）

### 2.3 L3 控制流层

1. **循环去糖**（扁平化前置）：
   - `for i=a,b,c do body` → `local i,_lim,_st=a,b,c; while true do i=i+_st; if (...) then break end; body end`
   - `for v in f(a) do body` → `do local _it,_s,_c=f(),a; while true do local v=_it(_s,_c); if v==nil then break end; body end end`
   - `repeat body until c` → `while true do body; if c then break end end`
   - `//`（仅 Luau 输入、5.1 目标）→ `math.floor(a / b)`（实测语义一致）
   - 去糖后程序只含：简单语句 / if / while / return / break / continue
2. **CFG 扁平化（control flow flattening）**——核心：
   - 把整个函数体拆成**基本块**（块 = 1 条简单语句 或 1 个条件表达式 或 汇聚点）
   - if/while 展开为块间边；条件块求值到临时变量后分支
   - 每块分配**随机状态 ID**（大整数），块顺序随机
   - 发射形态（5.1 安全，无 goto）：
     ```lua
     local _s = 482193
     while true do
       if _s == 482193 then
         local _c1 = <cond expr>
         if _c1 then _s = 117 else _s = 231 end
       elseif _s == 117 then
         <原语句>; _s = 905
       elseif _s == 905 then
         return x, y          -- return 直接发射（跳出 while）
       elseif _s == 771234 then
         break                -- 原 break/continue 变成指向对应块的边
       end
     end
     ```
   - **尾调用保真**：`return f(...)` 必须原样发射（不可先存局部变量，否则丢失尾调用优化语义）
   - 不可达块丢弃；条件为 `false`/nil 时分支走 else（不能用 `a and b or c` 模式）
3. **if 拆分（轻量档）**：单个 if 语句 → `do ... end` 内的小状态机（同一套 CFG 代码，作用域限制在该 if）
4. **透明谓词**：`if math.floor(math.abs(x)) >= 0 then ... end`（恒真/恒假，纯算术无副作用）
5. **垃圾代码**：函数开头/块间注入无副作用的随机算术赋值、空 `do end`、恒真假 if。
   约束：只碰新造的局部变量，不碰全局、不调用任何函数、无 I/O

### 2.4 L4 数值层

- 整数：拆成 2–4 个带符号小项之和/差（`17 - 6 + 31`），避免 0/1 平凡项
- 浮点：**只做精确恒等变换**（`0 + x`、`x - 0`、`x * 1`、`x / 1`）——
  任何加拆分都会改变浮点值，禁止
- 输出格式：整数用 `%d`（|v|<1e15 内），否则 `%.17g`；
  **Luau 的 int/float 字面量区分**：原字面量带 `.`/`e` 时输出必须保留小数点（`3.0` 不能变 `3`）

### 2.5 L5 整体加密层

- 全部 pass 跑完后，把整个源码作为一个大字符串用 L2 同一密码加密，
  切成多段（每段数 KB）字面量：
  ```lua
  <loader: key + _dec + 反篡改>
  local _f = loadstring(_dec(p1, p2, ..., p64))
  _f()
  ```
- **实测关键约束：Luau 没有全局 `load`，只有 `loadstring`；5.1 两者都有 →
  统一用 `loadstring`（双方言兼容，无需 fallback 分支）**

### 2.6 L6 VM 层 ★（商业级核心，对标 Luraph）

#### 2.6.1 总体结构

```
                [Rust 混淆器]
  Lua 源码 ──→ 解析/AST/符号表 ──→ 自定义字节码编译器 ──→ 加密字节码容器
                                                      │
输出 Lua 源码 =  ①  运行时加载器(解密容器)
             + ②  自定义解释器模板(Lua 源码，经 L1/L2/L3 混淆)
             + ③  加密字节码(分段字面量)
             + ④  入口: 初始化 VM 状态 → 执行主函数原型
```

标准反编译器拿到的只有「一堆数组 + 一个 while 循环解释器」——
**没有任何一条原生 Lua 字节码指令，也没有可读源码**。

#### 2.6.2 指令集（ISA）设计草案

采用**寄存器 VM**（与 5.1 语义最对齐，实现最不易出错），约 40 条指令：

| 组 | 指令 |
|---|---|
| 加载 | `LOADK`(常量) `LOADNIL` `LOADBOOL` `MOVE` `VARARG` |
| 取值 | `GETLOCAL` `GETUPVAL` `GETGLOBAL` `GETTABLE` `GETINDEX` |
| 赋值 | `SETLOCAL` `SETUPVAL` `SETGLOBAL` `SETTABLE` `SETINDEX` |
| 算术 | `ADD` `SUB` `MUL` `DIV` `MOD` `POW` `CONCAT` `LEN` `UNM` `NOT` |
| 比较 | `EQ` `NE` `LT` `LE`（操作数可为寄存器/常量） |
| 跳转 | `JMP` `JZ` `JF`(条件为 falsy 跳) |
| 调用 | `CALL` `TAILCALL` `RETURN` `ERROR` `PCALL`(封装宿主 pcall) |
| 表 | `NEWTABLE` `CLOSURE`(嵌套原型 + upvalue 捕获) |

- 编码：`opcode(8bit) + A(8bit) + B(8bit) + C(8bit)` 打包为 32 位整数存入 SoA 数组
  （**借鉴 Luraph v14**：opcode/A/B/C 分存不同数组，破坏 AoS 特征）
- 常量池：字符串（加密）/数字/布尔/nil/函数原型指针
- 原型池：嵌套函数 = 原型树（指令流 + 局部变量名表 + upvalue 描述 + 常量表 + 子原型）
- **upvalue**：描述「来自哪个父级（局部 or upvalue）」，闭包创建时实例化槽
- **vararg**：原型带 `is_vararg`，`VARARG` 指令从协变参表取 `...`
- **环境**：v1 用宿主环境（全局直读直写）；5.1 `_ENV` 不存在问题；
  Luau `_ENV` 表语义天然兼容
- **元表/协程/pcall/io 等**：全部落到宿主 VM 原生对象上（不模拟，只透传）——
  保证行为 100% 与宿主一致，代价是元表操作仍可见（可接受，Luraph 同策略）

#### 2.6.3 解释器模板（Lua 源码，由 Rust 生成，每次构建不同）

```lua
local function _exec(proto, regs, ups, varargs)
  local pc = 0
  local e = proto.code      -- SoA: e=opcodes, u=Y=operands, H=constants
  while true do
    local d = e[pc + 1]
    -- 二分决策树派发（每次构建树形/阈值随机生成）：
    if d < T1 then
      if d < T2 then ... handler 0 ... else ... handler 1 ... end
    else
      ...
    end
    -- 无显式 pc+1：JMP/RETURN 直接改写 pc 或 return
  end
end
```

- **派发树每次构建随机**：比较阈值、分支顺序、handler 排列全随机
- **指令编码随机（VMC, VM Customization）**：
  每构建生成随机 opcode 置换表 + 每构建随机寄存器基址偏移 +
  随机插入**死指令**（永不命中的 handler 分支 / 无效操作数）
- **解释器自身的混淆**：生成后的解释器源码再过 L1（重命名）+ L2（字符串加密）+
  垃圾代码注入；关键表名（e/u/Y/L/H）随机化
- **反模拟**：
  - 容器**校验和**（字节和 mod 质数）与内嵌期望值比对，不匹配 → 陷阱
  - **时间陷阱**：解密+首段执行用 `os.clock()` 计时，超阈值（调试器单步）→ `while true do end`
  - 解释器内金丝雀函数：被 hook 检测（`pcall` 调用 + 返回值校验）
- **性能策略**：寄存器数组复用、handler 内联、避免每指令 `pcall`（错误用
  `xpcall` 只在 ERROR 指令处）；体积/速度作为预设参数（small/balanced/fast）

#### 2.6.4 与 Luraph 的差异/取舍

| 点 | Luraph | 本项目 v1 |
|---|---|---|
| 字节码格式 | SoA + 私有编码 | SoA（同款思路）+ 随机 opcode 置换 |
| 派发 | 二分决策树 | 二分决策树（随机阈值） |
| unrolled VM | 有（巨型顺序语句） | v2 再做（体积换性能的高级档） |
| 字符串解码器 | LZW+Base36 | L2 密码（add8/xor8）+ 分段，v2 可加压缩 |
| 每构建唯一 | ✅ | ✅（opcode 置换/派发树/名字/常量表顺序全随机） |
| 元表模拟 | 部分 | 透传宿主（v1 取舍：正确性优先） |

### 2.7 L7 反篡改层

- 容器校验和（L6 内嵌）
- 时间陷阱（解密段执行计时）
- 金丝雀：loader 内放一个「正常执行必返回特定值」的小函数，执行前自检
- 错误混淆：真实错误信息加密后抛出（防从报错里泄漏结构）

---

## 3. 双方言语义实测表（2026-08-23 实测，2026-08-25 补最后两行）

| 特性 | Lua 5.1.5 | Luau 0.735 | 对混淆器的影响 |
|---|---|---|---|
| 全局 `load` | ✅ | ❌ **不存在** | 运行时加载统一 `loadstring` |
| 全局 `loadstring` | ✅ | ✅ | 双方言共用 |
| `%` 语义 | floor（`-1%3=2`） | floor（同） | 解密算术双方言共用 |
| `//` floor 除法 | ❌ | ✅ `= math.floor(a/b)`（`-7//2=-4`） | 5.1 目标时去糖为 `math.floor` |
| goto / `::label::` | ❌ | （0.735 无） | 输入含 goto → 明确报错 |
| 位运算 `& │ ~` | ❌ | ❌（值级；仅类型系统用） | 输出永不产生位运算 |
| `continue` | ❌ | ✅ 上下文关键字 | 解析器按方言处理 |
| 复合赋值 `+=` 等 | ❌ | ✅ | 解析期去糖 `a = a + b`（LHS 重求值，文档注明） |
| 字符串插值 | ❌ | ✅ 反引号 `` `a {e} b` `` | 去糖为 `string.format`（`%`→`%%` 转义） |
| 类型注解 / `type X=` | ❌ | ✅ | 解析期剥离（子集语法，其余报错） |
| 类型注解/`type X=` | 无 | 有（解析期剥离；函数类型用 `->`；0.735 不支持花括号函数体 `{}`） |
| `\x` 字符串转义 | **非转义**（`\x41`=字面量 `x41`；未知转义 `\c`→字面量 `c`） | 是转义（`\xhh` 恰好 2 位，1 位报错） |
| `coroutine.close`/`isyieldable` | 无（5.2+ 才有） | 有 |
| 尾调用优化 | ✅ 解释器做 TCO | ⚠️ CLI 构建不做（共享语料递归限 5000 深） |
| `_VERSION` | "Lua 5.1" | "Luau" | 可做软方言自检（不硬失败） |
| `unpack` / `table.unpack` | unpack 全局 | 两者都有 | 工具自身/输出用 `select`+`{...}` 规避 |
| 整数/浮点类型区分 | ❌ 单一 number | ✅ int/float 独立类型 | AST Num 节点带 `isfloat` 标志保真 |
| `os.clock` | ✅ | ✅ | 时间陷阱双方言可用 |
| `math.floor` | ✅ | ✅ | `//` 去糖依赖 |
| for-in 裸 table `for k,v in t` | ❌ **迭代器必须可调用**（运行时
  `attempt to call a table value` = 正确行为） | ✅ **语言级扩展**（隐式
  `next, t`） | parser 在 Luau 档把单非调用迭代器归一化为 `next, t`；5.1 档
  原样透传（VM 与宿主同错）。共享语料两侧行为一致，可覆盖 |
| 顶层新建全局赋值 | ✅ 合法 | ❌ **报错**（官方 CLI 的 `luaL_sandbox`
  全局只读，0.600+ 均如此：`attempt to modify a readonly table`） | **共享
  语料不得含顶层新建全局赋值**（cross 检查必挂）；VM 的 SetGlobal 用
  `getfenv(0)` 天然同错（语义镜像正确） |

### 运算符优先级（核对 5.1 `lparser.c` 与 Luau `Parser.cpp` 源码，两者一致）

```
or(1) and(2) 比较(3) ..(5,右) +- (6) * / % //(7→乘法级) 一元(8) ^(10,右,指数取一元级)
```
- Pratt 表驱动：`subexpr(limit)`，左优先 > limit 才结合，右操作数用右优先递归
- 关键易错点：`-2^2 = -(2^2)`（一元 8 < ^ 左 10）；`2^-3` 合法（指数是一元级）；
  `not x and y = (not x) and y`；`1..2..3 = 1..(2..3)`
- 解析器按此表实现，测试语料必须包含上述全部组合

---

## 4. 关键实现约束与坑清单

1. **输出必须双方言可重解析**：每次输出先过自家解析器（自校验），再过目标解释器
   （`lua51 -e "loadstring(...)"` / `luau-compile`）做语法级验证
2. **多值赋值**：`a, b = f()` 尾部展开规则；扁平化的块发射不能拆散多赋值
3. **尾调用**：`return f(...)` 原样保留（2.3）
4. **局部可见性**：声明后的下一条语句起可见；`local function` 体内不可见自身；
   `for` 变量作用域限制在循环体
5. **字符串字节语义**：`\0` 合法（用转义字面量输出）；`#` 取长度；
   长字符串 → 统一转短串字面量（`[[` 内换行/括号要转义）
6. **浮点保真**：`%.17g` + Luau float 字面量补 `.0`（2.4）
7. **`self` 与元方法名**：`__index`/`__call` 等是字符串键，L2 加密后运行时还原，
   不影响元表机制
8. **协程**：`coroutine.resume` 跨函数传多值——VM 的 RETURN 多值协议必须正确
   （nresults 约定：0=1 个结果、-1=全部）
9. **Rust std-only**：无 serde/clap → 手写 CLI 参数解析与 AST 序列化（不需要，
   字节码直接在内存中生成）
10. **确定性**：所有随机性走种子 PRNG（`--seed` 可复现），PRNG 用纯算术
    （5.1/5.3/Luau 行为一致，便于对照测试）

---

## 5. Rust 实现规划（M0–M6 已落地，2026-08-25 现状）

```
luraph-rs/
├── Cargo.toml              # 零依赖（std-only，沙箱网络约束）
└── src/
    ├── main.rs             # CLI：参数解析（preset 命名待 M6）+ 管线装配
    ├── rng.rs              # 种子 PRNG（Park-Miller，rng_check.rs 对拍）
    ├── lexer.rs            # 双方言词法（转义/长串/反引号插值）
    ├── parser.rs           # 双方言语法（Pratt/注解剥离/插值/复合赋值去糖；
    │                       #   Luau for-in 隐式 next 归一化在这）
    ├── ast.rs              # AST 节点 + 克隆 + 遍历工具
    ├── symtab.rs           # resolve pass：作用域/sym 对象/全局名收集
    ├── printer.rs          # AST → 源码（优先级括号/字节精确字符串；
    │                       #   Ctx::Suffix 后缀位置括号——(5).nope 不得打成 5.nope）
    ├── minify.rs           # L1 token 感知单行压缩（默认开）
    ├── mangle.rs           # L1 名称混淆
    ├── strings.rs          # L2 字符串加密 + 加载器生成
    ├── flatten.rs          # L3 CFG 扁平化（块图构建 + 状态机发射 + 循环降级
    │                       #   make_loop——ForGen 单迭代器语义在这）
    ├── junk.rs             # L3 垃圾代码/透明谓词
    ├── numbers.rs          # L4 数值混淆
    ├── body.rs             # L5 整体加密（loadstring 容器）
    ├── antidbg.rs          # L7 校验和/时间陷阱/错误重写/金丝雀
    ├── desugar.rs          # ⚠️ 孤儿文件（未挂 mod，死代码存档——勿依赖）
    └── vmgen/              # ★ L6 VM
        ├── mod.rs
        ├── isa.rs          # 41 条 Op + 每构建随机置换 OpMap + 变长 varint 编码
        ├── compiler.rs     # AST → 字节码（CellKind 单 cell 模型：Plain/Slot 0x8000/
        │                   #   Up 别名 0xC000；5.1 构造器存储序方言分支）
        └── template.rs     # Lua 解释器模板（随机决策树分派/makefn/Nop 死指令/
                            #   slot_perm 槽位随机/元表·协程·pcall 透传）
tests/
├── cases/*.lua             # ★ 29 个语料（20 共享 + 9 luau_*；8 个 stress_*）
├── run_tests.sh            # ★ 官方矩阵（204 项：非 VM 102 + VM 102，含 5.1→luau 交叉）
├── multiseed.sh            # ★ 多种子回归（VM 改动必跑；seeds 可传参）
└── gen_examples.sh         # 混淆示例再生成
tools/luau-cli-mains/       # 重建 .tools 的 luau/luau-compile 自写 main（权威副本）
```

**验收标准（商业级定义落地，当前状态 2026-08-25）**
1. 全语料 × 双解释器 × 全预设 100% 输出一致（stdout + 退出码）✅ 矩阵 204/204
2. VM 预设输出经 `luau-compile`/`lua51 loadstring` 语法校验 0 错误 ✅（矩阵内含）
3. 同一 `--seed` 两次构建输出逐字节一致；不同 seed 字节码编码完全不同 ✅
   （前 2KB 重合率 8.6% 已验证）；**多种子回归**（≥5 seeds）0 失败 ✅
4. 对 VM 预设输出：标准反编译器/格式化器无法恢复源码结构（人工抽查）✅ M5
   （`luac51 -l` 仅见 L5 容器；用户明文不可检索）
5. 反篡改生效：篡改任一密文段 → 触发陷阱 ✅（M3 实测）

---

## 6. 测试语料清单（所有常用语法，强制覆盖）

> 每次混淆模块改动后，以下语料必须全部通过 `lua51` + `luau` 双方言
> 语法校验（`loadstring`/`luau-compile`）+ 运行对比（stdout/退出码一致）。
> 语料文件：`luraph-rs/tests/cases/*.lua`（**已建立，29 个**：20 共享
> 5.1+Luau 双跑 + 9 个 `luau_*` 前缀专属用例；其中 8 个 `stress_*` 是
> M4 续期新增的应力用例——upvalues/coroutines/metamethods/multival/
> errors/bigtable/control 共享 + luau_vm 专属），Luau 专属用例单独标记。
> **共享语料红线**（cross 检查要求原始程序两侧行为一致）：不得含顶层
> 新建全局赋值（Luau 沙箱报错）、重复键表构造器（5.1/Luau 存储序不同）、
> 未捕获错误的裸报错（行号格式不同）——详见 HANDOFF §8 第 15 条。

1. **运算符与优先级**：全部二元/一元运算符；易错组合 `-2^2`、`2^-3`、
   `not x and y`、`1..2..3`、`a and b or c`、`#t ^ 2`
2. **控制流**：if/elseif/else、嵌套 if、while、repeat-until、数字 for（含负步长/无步长）、
   泛型 for（pairs/ipairs/自定义迭代器）、do 块、break 在多层嵌套各位置
3. **函数**：local/global/方法（`t:m`）定义、嵌套闭包、upvalue 读/写、递归、
   互相递归、匿名函数、变参（`...`、`select`、`unpack`）、尾调用
   （`return f(...)`、`return ...`）、`local function` 自引用（不可见语义）
4. **表**：`{}`、`{a=1}`、`[k]=v`、混合数组/哈希、多维索引、方法调用链、
   `table.insert/remove/sort/concat/maxn`
5. **元表**：`__index`（表/函数）、`__newindex`、`__call`、`__add` 等算术 metamethod、
   `__tostring`、`__len`、`setmetatable`/`getmetatable`/`rawget`/`rawset`
6. **字符串**：单/双引号、全部转义（`\n \t \r \a \b \f \v \\ \" \' \ddd \xhh`、
   含 `\0`）、长字符串 `[[ ]]` / `[==[ ]==]`、换行续行、`string.format/sub/rep/
   gmatch/gsub/match/find/byte/char`
7. **数字**：整数、浮点、十六进制 `0x1F`、科学计数 `1.5e-3`、`#` 取长、
   浮点边界（`0.1+0.2`、`math.huge`、`-0`、`1e308`）、`math.floor/abs/sqrt/random`
8. **多值语义**：`a,b = f()`、`local a,b = f()`、`return a, b`、`{f()}` 尾展开、
   `a and b or c` 多值、`unpack(t)`、`#f()`
9. **标准库/运行时**：`print`、`error`+`pcall`/`xpcall`、`tostring/type/tonumber`、
   `os.time/clock/date`、`coroutine.create/resume/yield/wait`（含跨协程多值）、
   `select`
10. **空/边界**：空函数体、空 if 分支、空串 `""`、单语句脚本、`return`（无值）、
    顶层表达式语句
11. **Luau 专属**（仅 luau 目标验证）：`continue`、复合赋值 `+= -= *= /= //= %= ^=`、
    `//` 负数、反引号插值（含 `\{` 转义、多表达式、无占位符）、类型注解（局部/参数/
    返回/泛型/联合/交叉/表类型）、`type X = ...` 别名、`export type`
12. **风格脚本**：模拟 Roblox 事件连接（`Connect`/`Disconnect` 闭包模式）、
    游戏主循环、require/module 模式（`local M = {} return M`）

---

## 7. 参考资料

0. **Luraph 15 混淆脚本（用户提供 `samples/luraph15.txt`）**：✅ 分析完成，
   报告见 `docs/luraph15-analysis.md`
1. Luraph 官方资料与 Grokipedia 综述（架构/历史/宣称）
2. vfxecho/obfuscated-lua —— Luraph v14 逆向分析（SoA 格式/决策树/unrolled VM/LZW 解码器）
3. CodePal《Luraph Deobfuscator Guide》（分层模型、VM hooking 思路）
4. Aback Tools 2026 混淆器对比（VM 虚拟化原理、防护级别划分）
5. Lua 5.1 Reference Manual + 5.1.5 源码 `lparser.c`（优先级表/语法）
6. Luau 0.735 源码 `Ast/src/Parser.cpp`、`Ast/src/Lexer.cpp`（优先级/关键字/
   插词法/无 goto 无位运算 实证）
7. 本仓库 `docs/` 内全部实测记录（lua5.1 5.1.5 / luau 0.735 真实解释器）
