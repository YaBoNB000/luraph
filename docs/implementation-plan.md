# 实施计划（IMPLEMENTATION PLAN）

> 回答两个问题：**将实施哪些混淆方法** + **目前到哪了**。
> 活文档：**每完成一个里程碑（或混淆 pass）必须同步更新 §1 表格状态列 +
> §2 进度表 + §3 里程碑勾选 + 更新日志**（四处一起改，不允许只记日志不改状态），
> 并同步 `PROGRESS.md` / `HANDOFF.md` / `examples/README.md`，
> **最后 commit + push 到 GitHub**（用户规矩 2026-08-23）。
> 初版：2026-08-23 ｜ 最后更新：2026-08-23（M2 完成 + L1 minify 补齐）
> **当前状态：M0 ✅ + M1 ✅（L1 全量：mangle+minify / L2 字符串加密）+ M2 ✅（L3 控制流扁平化已实现），下一步 M3 数值+整体加密+反篡改**

---

## 1. 实施清单（全部混淆方法，按层分组）

> 状态图例：📐 设计完成 ｜ 🟡 部分设计 ｜ ⬜ 未开始 ｜ ✅ 已实现
> 优先级：P0 = 商业级预设必需 ｜ P1 = 增强 ｜ P2 = 可选/延后

### L1 词法层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 名称混淆（局部/参数/upvalue/循环变量） | 作用域安全的随机重命名（符号表驱动，避开关键字/全局名/互影；隐式 `self` 固定名） | `mangle.rs` | P0 | ✅ M1 |
| 随机命名风格混合 | 短名(2-4)/中名(5-8)/长名(9-15) 按比例混用（30/40/30%） | `mangle.rs` | P1 | ✅ M1 |
| minify / 空白压缩 | **token 感知单行压缩**：复用词法器重发射，只在「双标识符边界 / `--` 成注释 / `1..2` 成坏浮点 / 数字-`..`-数字」等边界插空格（默认开，`--no-minify` 保留缩进形态） | `minify.rs` | P0 | ✅ 2026-08-23（19 单测 + 全语料双解释器验证） |

### L2 字符串层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| add8 算术密码（默认） | `enc=(b+key[i%24]+i)%256`，5.1/Luau 共用解密算术（% 同为 floor，实测） | `strings.rs` | P0 | ✅ M1 |
| XOR 密码 | Luau 用 bit32.bxor；**5.1 用 256×256 查找表 XOR**（双方言同一密钥派生） | `strings.rs` | P1 | ⬜ |
| 位置相关密钥流 | `key[i%24] + i`（位置+下标双重相关，每构建随机密钥） | `strings.rs` | P0 | ✅ M1 |
| 密钥流源头 = LCG PRNG（状态机化） | mod 2²⁸，常数每构建随机，PRNG 发射成状态机（luraph15 同款） | `rng.rs`+`strings.rs` | P0 | 🟡 工具侧 PRNG 已有（M1 用于生成密钥）；「输出内 PRNG 状态机化」未做 |
| 字符串拆分（unrolling） | 密文切 3 段字面量，运行时 concat | `strings.rs` | P0 | ✅ M1 |
| 密钥碎片化嵌入 | 3 段拼接 + 运行时展开为字节表（输出无连续密钥） | `strings.rs` | P1 | ✅ M1 |
| 全量字符串覆盖 | 全部 Str 节点（含表键/索引/插值模板）→ 解密调用 | 全 pass | P0 | ✅ M1 |

### L3 控制流层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 循环去糖 | ~~for/for-gen/repeat → while~~ **取消**：flatten 原生处理所有循环（块图直接收 for/repeat/while + break/continue 边），且旧去糖有 continue 语义缺陷 | ~~`desugar.rs`~~（存档） | — | ❌ M2 取消 |
| CFG 扁平化（函数级状态机） | 块图 → 随机状态 ID + 随机分支序 + `while true` 分派（无 goto 形态）；**循环=嵌套子状态机**（循环体局部变量每轮 fresh，闭包捕获语义与原生 for 一致） | `flatten.rs` | P0 | ✅ M2 |
| if 拆分（轻量档） | 单 if → do/end 内小状态机（同一套块图代码）——并入主扁平化：每个 if 即 cond 块+分支块 | `flatten.rs` | P1 | ✅ M2 |
| 透明谓词 | 恒真算术条件 if（`b*b >= 0`） | `junk.rs` | P1 | ✅ M2 |
| 垃圾代码注入 | 无副作用随机算术块（只碰新局部变量；修复了跨作用域引用 c 与 return 后注入两个 bug） | `junk.rs` | P1 | ✅ M2 |
| Luau 专属去糖 | `//`→math.floor（AST Idiv 节点，打印时去糖）、复合赋值→普通赋值、反引号插值→string.format | `parser.rs` | P0 | ✅ M0（解析期完成） |
| 类型注解/别名剥离 | 解析期丢弃（含函数类型 `->`/表类型/泛型/交集） | `parser.rs` | P0 | ✅ M0 |

### L4 数值层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 整数拆分 | 2-4 个带符号小项之和/差（避免 0/1 平凡项） | `numbers.rs` | P1 | ⬜ |
| 常量补码编码 | 大基数减法（`85523 - q` 风格，luraph15 同款） | `numbers.rs` | P1 | ⬜ |
| 浮点精确恒等变换 | 仅 `0+x / x-0 / x*1 / x/1`（任何拆分都改值，禁止） | `numbers.rs` | P1 | ⬜ |
| 浮点输出保真 | `%.17g`（Rust {:?} 最短往返）；Luau float 字面量补 `.0`；isfloat 标志；inf/NaN 特判 | `printer.rs` | P0 | ✅ M0 |

### L5 整体加密层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 全源码加密 + loadstring | 所有 pass 后整段加密切分，`loadstring(_dec(...))()`（实测双方言用 loadstring） | `body.rs` | P0 | ⬜ |

### L6 自定义 VM 层（核心护城河）

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| ISA 设计（~40 条寄存器指令） | 加载/取值/赋值/算术/比较/跳转/调用/表/闭包 | `vmgen/isa.rs` | P0 | 📐 |
| SoA 字节码容器 | opcode/操作数分存平行数组（luraph14/15 同款，破坏 AoS 特征） | `vmgen/isa.rs` | P0 | 📐 |
| opcode 随机置换（每构建） | 随机置换表 + 死指令填充 | `vmgen/isa.rs` | P0 | 📐 |
| AST → 字节码编译器 | 原型树/upvalue/常量池/多值协议(nresults 0/-1)/尾调用 | `vmgen/compiler.rs` | P0 | 📐 |
| 解释器模板生成（Lua 源码） | fetch-decode-execute + 随机二分决策树（2~4 层） | `vmgen/template.rs` | P0 | 📐 |
| continue 扁平分派风格（Luau 档） | 叶 = `状态=handler(); continue`（luraph15 同款，5.1 用嵌套树） | `vmgen/template.rs` | P1 | 📐 |
| 帧运行器入场原语解包 | 数字槽→局部变量一次性解包，槽号/局部名每构建随机 | `vmgen/template.rs` | P0 | 📐 |
| 普通/协程双帧运行器 + UL 跨 yield 传表 | 协程帧走 coroutine.wrap + setfenv；大结构跨 yield 成对传送 | `vmgen/template.rs` | P0 | 📐 |
| 7-bit 分块操作数编码 | 7/14/21-bit 变长 + 128 进制重建 + 2³² 归一化（VM 档可选开） | `vmgen/compiler.rs`+`template.rs` | P1 | 📐 |
| 状态元组位置传参（混函数引用） | 状态含原语指针，每构建顺序随机 | `vmgen/template.rs` | P0 | 📐 |
| 解码枢纽状态 + 私有解码辅助 | 解码主循环枢纽 ID 每构建随机；辅助函数运行器内私有 | `vmgen/template.rs` | P0 | 📐 |
| base-N 编码 + token 转义 | 字符类 ASCII 32..126 + 10 特殊字符→5 字符 token（pC 同款） | `vmgen/isa.rs` | P0 | 📐 |
| 元表/协程/pcall 透传宿主 | 不模拟，正确性优先 | `vmgen/compiler.rs` | P0 | 📐 设计定稿 |
| 解释器自身再过 L1/L2/垃圾注入 | 模板生成后走同一套 pass（表名随机化） | 管线 | P0 | 📐 |
| 每帧新 Lua 闭包（CPS 帧模型） | 性能代价大 | — | P2(v2) | ⬜ |
| 超级指令（算术+比较+分支融合） | 编码复杂度高 | — | P2(v2) | ⬜ |

### L7 反篡改层

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 容器校验和 | 字节和 mod 质数 vs 内嵌期望值，不匹配→陷阱（**v15 没有，我方差异化**） | `antidbg.rs` | P0 | ⬜ |
| 时间陷阱 | 解密段 os.clock 计时超阈 → `while true do end`（v15 没有，差异化） | `antidbg.rs` | P1 | ⬜ |
| 错误重写状态机 | 行号解析(`:(%d+)[:\r\n]` 同款)+ level 调整重抛（luraph15 nL 同款） | `antidbg.rs` | P1 | ⬜ |
| 金丝雀自检 | 正常执行必返回特定值的小函数，执行前校验 | `antidbg.rs` | P2 | ⬜ |

### 平台 / CLI

| 方法 | 说明 | Rust 模块 | 优先级 | 状态 |
|---|---|---|---|---|
| 双方言模式 | `--dialect 5.1\|luau`（解析/去糖/输出已通；VM 载体待 M4） | 全模块 | P0 | ✅ M0（解析/输出链），VM 部分待 M4 |
| 种子确定性 | `--seed`（同 seed 逐字节一致，不同 seed 编码完全不同；默认时间种子） | `rng.rs` | P0 | ✅ M1 |
| 预设 | low(L1+L2) / medium(+L3) / high(+L4+L5+L7) / vm(全开) / max(v2 特性) | `main.rs` | P0 | 🟡 单开关已有（--no-mangle/--no-strings/--no-flatten/--no-junk，M1+M2），预设命名待 M6 |
| 强制测试工作流 | 每 pass 完成 → 混淆样本 → lua51+luau 语法+运行对比（语料 21 个文件） | `run_tests.sh`+`gen_examples.sh` | P0 | ✅ M0 起执行中（当前 68 项检查全绿） |

---

## 2. 目前到哪了（2026-08-23）

| 阶段 | 进度 | 说明 |
|---|---|---|
| 环境（Rust 1.88 / lua51 5.1.5 / luau 0.735） | ✅ 100% | 路径见 HANDOFF.md §4 |
| 调研 + 双方言语义实测 | ✅ 100% | `docs/obfuscation-research.md` |
| Luraph v15 分析（含动态分析共三轮） | ✅ 100% | `docs/luraph15-analysis.md`（§11 25 项功能对比：采纳 20 / 延后 2 / 拒绝 2 / 我方独有 1） |
| 全部混淆方法设计（L1–L7 + VM） | ✅ 100% | 本文件 §1 + research §2.6 + v15 报告 §8/§10 |
| **Rust 混淆器代码** | ✅ **M0+M1+M2（3/7 里程碑）** | M0 地基（lexer/parser/symtab/printer）+ M1 混淆（L1 mangle + L2 strings）+ M2 控制流（L3 flatten 状态机 + junk，desugar 取消） |
| 测试语料 + 矩阵 | ✅ 100% | 21 个语料文件（13 个共享 + 8 个 Luau 专属，含 loops/luau_loops）；68 项检查全绿 |
| 混淆示例 | ✅ M1+M2 产出 | `luraph-rs/examples/`（21 个文件，`tests/gen_examples.sh` 再生成） |

**一句话：设计 100%，代码 M0+M1+M2 完成（3/7 里程碑，矩阵 68/68 全绿），下一步 M3 数值+整体加密+反篡改。**

---

## 3. 里程碑（说「开始」后的执行顺序）

| 里程碑 | 内容 | 验收标准（含强制工作流） |
|---|---|---|
| **M0 地基** ✅ 2026-08-23 | `cargo new` + rng/lexer/parser/ast/symtab/printer | 全语料 round-trip × 双解释器 0 错误 —— 通过 |
| **M1 词法+字符串** ✅ 2026-08-23 | mangle(L1) + strings(L2) | 混淆样本双方言运行一致；明文字符串=0（除 loader 必要项）—— 通过 |
| **M2 控制流** ✅ 2026-08-23 | flatten + junk(L3)（desugar 取消：flatten 原生处理 for/repeat/while/continue/break） | 同上；人工抽查输出为状态机形态、无原结构残留 —— 通过（矩阵 68/68；循环闭包捕获/continue/空区间 for/nested break 均有语料覆盖） |
| **M3 数值+整体+反篡改** | numbers(L4) + body(L5) + antidbg(L7) | 同上；篡改密文段→触发陷阱 |
| **M4 VM 最小可用** | vmgen: isa + compiler + template（最小 ISA 覆盖语料子集） | `--preset vm` 输出双方言运行一致；无原生字节码可读结构 |
| **M5 VM 完整** | 全指令 + VMC 随机面（置换/树形/命名/枢纽 ID）+ 7-bit 档 | 同 seed 逐字节一致；异 seed 编码完全不同；反编译器人工抽查失效 |
| **M6 产品化** | CLI 预设打磨 + README 产品文档 + 性能数据 | 全预设 × 全语料 × 双解释器 100% 通过（research §5 验收 5 条） |

**M3 是当前唯一待启动项。**

---

## 4. 更新日志

- **2026-08-23（L1 minify 补齐）** `minify.rs`：输出层 token 感知单行压缩
  （L1 词法层至此全量）。实现 = 用目标方言词法器重 lex 打印器输出再按
  最小空白规则重发射：默认零空格，仅在 ① 双标识符/关键字/数字边界
  （`localx`、`else if`→`elseif`、`1end`…）② `-`+`-`（`a - -b`→`--`
  成注释吞行，edge 语料真实暴露）③ `..` 与数字/`.` 相邻（`1..2` 被
  Luau 当坏浮点，basics 语料真实暴露）时插空格。字符串按
  `print_string_bytes` 字节精确重编码。默认开（`--minify`/`--no-minify`）。
  踩坑记录：①`- - 5`→`--5` 注释吞掉整行 ②`1..2`→Malformed number
  ③Rust `{:?}` 数字归一化（`1.5e10`→`15000000000.0`，与打印机一致，值不变）。
  单测 19/19（含 token 序列不变、`--`/`..` 边界、幂等性），矩阵 68/68 全绿，
  --no-flatten 全语料 + 同 seed 复现 通过。
- **2026-08-23（M2 完成）** flatten(L3) + junk(L3) 完成，**desugar 取消**：
  flatten 直接以块图处理 for/repeat/while/continue/break。核心设计：
  ① 每个函数体=一个 `while true` + 随机 ID + 乱序分支的状态机；
  ② **循环=嵌套子状态机**——循环体块成为独立分派（循环节点的不透明块），
  循环体局部变量（含 for 循环变量）声明在 per-pass 作用域（每轮重新
  `local`），闭包捕获循环变量的语义与原生 for 完全一致（每轮 fresh）；
  ③ break/continue 是纯图边：continue→体尾（for/while=重入=增量/重判条件，
  与 Luau continue 语义一致；repeat 的 until 检查块在内部分派内，
  repeat+continue 也正确）；④ for-numeric 按原生顺序执行：区间检查在
  体前（空区间 0 次迭代）、循环变量每轮 fresh、增量在体后；
  ⑤ 作用域安全提升：被其他分支/跨分支闭包/循环体引用的 local 提升到
  机器顶部（mangle 保证名字与全局名不相撞，提升不会遮蔽全局）。
  修复的既有 bug：junk 的 `d = c+1-1` 跨作用域引用 c（c 在 if 内声明）、
  junk 在 return 后注入（Lua 语法错误）、旧 flatten 的 in_loop 判定把
  循环前 init 块局部变量整体丢弃（变全局引用 → nil 运算）。
  语料新增 `loops.lua`（共享）+ `luau_loops.lua`（Luau continue 边界）。
  矩阵 68/68 全绿（含 --no-flatten 路径全语料通过）。
  下一步：M3 = numbers(L4) + body(L5 整体加密) + antidbg(L7)。
- **2026-08-23（M1 完成）** mangle(L1) + strings(L2) 完成：名称混淆（含
  self keep_name 规则）、加性密钥流字符串加密（密钥 3 段拆分）、17 个语料的
  混淆示例生成到 `luraph-rs/examples/`。矩阵 62/62 全绿。新发现：local function
  自引用双方言可见；转义数字合并陷阱；FuncDecl 点链对象需走 symtab。
  下一步：M2 = L3 控制流（desugar + CFG 扁平化 + junk）。
- **2026-08-23（M0 完成）** Rust 骨架 + lexer/parser/symtab/printer 完成；
  17 个测试语料 × 双方言 62 项检查全绿（round-trip 语义等价验证通过）；
  新增方言发现：`\x` 转义差异、5.1 无 coroutine.close、Luau CLI 无 TCO、
  0.735 无花括号函数体。下一步：M1 = mangle(L1) + strings(L2)。
- **2026-08-23（第二轮）** luraph15 动态分析完成（样本真实运行于 Luau CLI）：
  四段式流水线角色分工验证、变长指令动态确认、~190 万 op/s 性能基线。
  设计新增：每段独立分发循环、1~2 个死代码分发器（VMC 随机面）、
  fetch 点数量每构建随机。字节级格式重实现判定为非必需（v3 可选）。
- **2026-08-23** 初版：实施清单定稿（吸收 luraph15 两轮分析的全部采纳项）；
  进度 = 设计 100% / 代码 0%。
