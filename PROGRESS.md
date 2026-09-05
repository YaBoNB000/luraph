# 项目进度（PROGRESS）

> 最后更新：2026-08-29
> 当前状态：✅ 环境 ✅ 研究 ✅ v15 分析 ✅ **M0–M6 全部完成**
> （M6：`--preset low|medium|high|vm|max` + 产品 README + `docs/performance.md`；
> 官方矩阵 **204/204** + 预设矩阵 **405/405** + 多种子 0 失败）
> **🟡 v15 结构同族（路线 A 已拍板）**：二轮详读订正计划 + P0 脚手架 +
> P1 外壳换代 + P2 模块表铺开 + P3 增量 1（CPS 骨架）+ 增量 2（staging
> 细拆 + 校验陷阱）+ 增量 3（Nop 别名自修改 + 死循环段 + 双数字槽
> 运行器）+ 增量 4（字面量自修改 + 分号发射 + v15 命名族）+ 增量 5
> （顶层机族扩容：三层分发同构）+ 增量 6（P4 载体密钥流混淆）+
> **安全增量 S1（密钥流状态机化）+ 阶段 B（解码状态机化）+ 阶段 C
> （执行 CPS 化：真 TCO + 内联 Call 系，深尾递归不溢栈）+ 阶段 A
> （操作数散布：寄存器/常量/upvalue 随机槽，静态恢复失锚）**完成；
> 随后用户改拍板「先结构 100% 还原」→ **结构战役增量 E1–E5 全部完成：
> 32/32 指纹 × 30 语料 × 5 种子全过（复合赋值折叠 / 宽参数 / if 表达式 /
> 内联 -128 阶梯 / RC+HB 长串 / 字符串池隐匿 / 规模地板）**，全语料 30 个
> v15 示例入库。下一步回到安全增强（阶段 D 每帧闭包等）。
> 细节见 `docs/v15-pipeline-rewrite.md`

## 0. 需求（用户确认）

- 商业级 Lua 混淆器，对标 Luraph
- **必须包含自定义 VM + 自己的解释器**（字节码虚拟化，核心卖点）
- 支持双方言目标：**Lua 5.1 / Luau**
- 混淆器本体用 **Rust** 编写
- 在用户明确说「开始」之前，不编写混淆器代码
- **【强制工作流】每次更改完混淆模块后**：
  1. 产出一个混淆脚本（用测试语料跑当前全部已实现 pass）
  2. 用 `lua51` 和 `luau` 两个解释器做语法 + 运行验证
     （`lua51 -e 'assert(loadstring(s))'` / `luau-compile`，并实际执行对比输出）
  3. 测试语料必须覆盖**所有常用语法**（见 `docs/obfuscation-research.md` 第 5 节语料清单，
     随模块增长持续补充），任何一步不通过 = 该模块改动未完成
  4. **同步更新所有相关 md**（用户 2026-08-23 强调）：
     `docs/implementation-plan.md`（§1 状态列 + §2 进度表 + §3 里程碑 + 更新日志）+
     `PROGRESS.md` + `HANDOFF.md`（如有变化）+ `luraph-rs/examples/README.md`
  5. **commit + push 到 GitHub**（历史分叉时先 merge 远端，勿 force push）
     —— 不更新 md / 不推 GitHub = 该模块改动未完成
- 用户提供的 **Luraph 15 混淆脚本**（`samples/luraph15.txt`，从 origin/main 拉取）：
  ✅ **分析完成** → `docs/luraph15-analysis.md`（三层分发/7-bit 分块寄存器/
  LCG 状态机密钥流/buffer 数据层/142 handler/脱壳评估/对本项目的设计启示）

## 1. 已实现内容

### 1.1 验证环境（✅ 全部就绪）

| 组件 | 版本 | 位置 | 用途 |
|---|---|---|---|
| Lua 5.1 解释器 | 5.1.5（源码编译） | `/home/user/luraph/.tools/bin/lua51`（仓库内，抗重置） | 验证 5.1 目标输出正确性 |
| luac 5.1 | 5.1.5（源码编译） | `/home/user/luraph/.tools/bin/luac51`（仓库内，抗重置） | 5.1 字节码编译检查 |
| Luau 解释器 | 0.735（g++ 直编，仓库内） | `/home/user/luraph/.tools/bin/luau`（抗重置；自写 main 复刻 CLI 环境：loadstring/require/luaL_sandbox） | 验证 Luau 目标输出正确性 |
| Luau 编译器 | 0.735（g++ 直编，仓库内） | `/home/user/luraph/.tools/bin/luau-compile`（抗重置） | Luau 输出语法校验 |
| Luau 分析器 | —（0.735 重建未编 luau-analyze，矩阵不需要） | — | 可选 |
| Rust 编译器 | 1.88.0 stable | `/home/user/luraph/.tools/lib/rustc/`（`/home/user/luraph/.tools/bin/rustc`，仓库内抗重置） | 混淆器本体编译 |
| Cargo | 1.88.0 | `/home/user/luraph/.tools/lib/cargo/`（`/home/user/luraph/.tools/bin/cargo`，仓库内抗重置） | Rust 构建 |
| gcc / g++ | Debian 12 (gcc 12) | 系统 | 编译了上述 C/C++ 解释器 |

> ⚠️ Rust 工具链是通过 npm 上的 `@rustbin` 预编译包安装的（rust-lang.org / rustup / crates.io
> 均被沙箱网络屏蔽）。**后果：Rust 项目必须 std-only（零第三方 crate 依赖）**——
> 对编译器类工具这是可接受的设计，且有利于商业闭源构建（无供应链依赖）。

### 1.2 双方言语义实测（✅ 用真实解释器逐条验证）

关键结论（详见 `docs/obfuscation-research.md` 第 3 节）：

- Lua 5.1：`%` 为 floor 语义（`-1 % 3 == 2`）；`loadstring` 存在；无 goto/位运算/`//`/continue
- Luau 0.735：
  - **无全局 `load`**，但有 `loadstring`（两者共有 → 运行时加载统一用 `loadstring`）
  - `%` 同样是 floor 语义 → 与 5.1 共用同一套解密算术，无需分支
  - `//` = `math.floor(a / b)`（floor 除法，`-7 // 2 == -4`）
  - **字符串插值语法是反引号**：`` `text {expr} text` ``（不是旧提案的 `("{}", x)`），
    字面量花括号用 `\{` 转义，`{{` 直接报错
  - **无 goto / 标签**（0.735 稳定版不支持），无值级位运算
  - `continue` 是上下文关键字（语句位置才生效）
  - 支持类型注解 / `type X = ...` 别名（解析时剥离即可）

### 1.3 混淆技术研究（✅ 完成）

- 系统调研 Luraph 及同类商业混淆器（含 Luraph v14 的公开逆向分析）
- 完成「分层混淆体系 + 自定义 VM 设计草案 + 实现规划」
- 成果文档：**`docs/obfuscation-research.md`**（本次学习的完整记录）
- **Luraph v15 样本三轮分析**（`docs/luraph15-analysis.md`）：
  第一轮（架构/解码流水线/VM/别名表/PRNG）+ 第二轮深挖（15 项新验证机制 +
  25 项功能对比表）+ **第三轮动态分析**（样本在 Luau CLI 上真实运行：
  buffer 内建于 CLI、5 个分发循环的角色分工、25 秒 4700 万 op、变长指令确认、
  引导 614 op 四段式流水线）
- **实施计划**（`docs/implementation-plan.md`）：L1–L7 每个方法的模块归属/
  优先级/状态 + 里程碑 M0–M6 验收标准

### 1.4 Rust 工具链（✅ 完成，hello world 构建运行通过）

## 2. 已写代码（过渡/参考代码，非 Rust，将来按 Rust 重写）

> 背景：最初以 Lua 实现做技术验证，用户确认改用 Rust 后暂停。
> 以下文件保留作为**语义参考实现**（语法边界、优先级、转义规则均已核对），
> 未做完整单元测试，**不会被 Rust 实现复用**（仅参考）。

| 文件 | 功能 | 状态 |
|---|---|---|
| `lph/rng.lua` | 种子化 PRNG（Park-Miller，纯算术，5.1/Luau 行为一致） | 完成，未测试 |
| `lph/lexer.lua` | 5.1 + Luau 词法分析器（数字/字符串/长字符串/转义/注释/反引号插值） | 完成，未运行 |
| `lph/parser.lua` | 5.1 + Luau 递归下降解析器（Pratt 表达式解析器、类型注解剥离、插值子解析、复合赋值） | 初稿，未运行 |

| 文件 | 说明 |
|---|---|
| `README.md` | 仓库标题（待更新） |

## 3. 进度与剩余工作

### ✅ M0 完成（2026-08-23，用户已发「开始」信号）

| 模块 | 文件 | 说明 |
|---|---|---|
| AST | `luraph-rs/src/ast.rs` | Expr/Stmt/Block/FuncDef；BinOp 含 Idiv（`//`） |
| 词法 | `luraph-rs/src/lexer.rs` | 双方言；全部转义 + 方言差异（`\x`/未知转义）；11 单测 |
| 语法 | `luraph-rs/src/parser.rs` | Pratt 优先级（与 lparser.c 一致）；注解剥离；插值/复合/`//` 去糖 |
| 作用域 | `luraph-rs/src/symtab.rs` | 可见性规则 / local function 自引用 / for 变量作用域 |
| 打印 | `luraph-rs/src/printer.rs` | 优先级括号（含 Ctx::Base 规则）/ 字节精确字符串 / 浮点保真 |
| PRNG | `luraph-rs/src/rng.rs` | Park-Miller（pass 阶段用） |
| CLI | `luraph-rs/src/main.rs` | `--dialect 5.1\|luau`、`-o` |
| 语料 | `luraph-rs/tests/cases/*.lua` | 17 个文件（12 类常用语法 + luau_* 专属 5 个） |
| 矩阵 | `luraph-rs/tests/run_tests.sh` | **62 项检查全绿**（语料 × 方言 × 运行对比/跨跑/luac） |

**M0 验收**：`cargo test` 11/11 + 矩阵 62/62 —— round-trip（输入→AST→输出）
在双解释器上语义等价。

**M0 阶段新发现的方言差异**（已进 lexer/parser 实现 + 研究笔记 §3）：
- `\x` 转义：5.1 中是字面量（`\x41`=`x41`，未知转义→字面字符）；Luau 中是转义（恰好 2 位）
- 5.1 无 `coroutine.close`/`isyieldable`（5.2+）→ 语料分离
- Luau CLI 构建无尾调用优化 → 共享语料递归深度限 5000
- Luau 0.735 无花括号函数体 `{}`（新语法）

### ✅ M1 完成（2026-08-23）

| 模块 | 文件 | 说明 |
|---|---|---|
| L1 名称混淆 | `luraph-rs/src/mangle.rs` | 局部/参数/循环变量/local function 名 → 随机名（短/中/长混合；避开关键字+程序全局名+互相碰撞）；**隐式 `self` 保持固定名**（`:m()` 语法绑定固定名 `self`） |
| L2 字符串加密 | `luraph-rs/src/strings.rs` | 全部字符串 → `dec(chunk1,chunk2,chunk3)` 运行时解密；加性密钥流 `enc=(b+key[i%24]+i)%256`（5.1 无位运算安全，双方言 `%` 均 floor——实测）；密钥 3 段拆分嵌入+运行时展开；解密器/密钥表名随机化 |
| 混淆示例 | `luraph-rs/examples/*.lua` | **17 个语料 × 对应方言的混淆输出**（`tests/gen_examples.sh` 重新生成） |

**M1 验收**：矩阵 62/62 全绿（混淆后输出在双解释器上与原代码输出一致）。

**M1 阶段新发现/修复**：
- `local function f` 自引用在 **5.1 和 Luau 都可见**（递归 local function 双方言可用；
  5.2 才改为不可见，我们两个目标方言都不受影响）→ symtab 先声明后解析函数体
- 隐式 `self` 参数不可重命名（`:m()` 固定绑定名 `self`）→ `Sym.keep_name` 标志
- 打印器转义陷阱：`\1` 后跟数字字节会被 re-lex 合并成 `\12` → 后随数字时强制
  3 位零填充 `\001`
- `function V.fn` 点链对象必须是 Expr（过 symtab/mangle），不能是裸字符串
- 匿名函数参数必须进 symtab（之前漏了，参数会逃过混淆）

### ✅ L1 minify 补齐（2026-08-23，M1 域最后一个 pass）

| 模块 | 文件 | 说明 |
|---|---|---|
| L1 minify | `luraph-rs/src/minify.rs` | 输出层 **token 感知单行压缩**：用目标方言词法器重 lex 打印器输出，再按最小空白规则重发射（默认开，`--no-minify` 保留缩进形态）。只在真正的危险边界插空格：① 标识符/关键字/数字相邻（`localx`/`elseif`/`1end`）② `-`+`-`（`a - -b` 会黏成 `--` 注释吞掉整行）③ `..` 与数字或 `.` 相邻（`1..2` 会被当坏浮点，Luau 直接拒绝）。字符串按 `print_string_bytes` 字节精确重编码。全语料输出现为单行 Luraph 式紧凑形态 |

**minify 验收**：单测 19/19（token 序列不变、`--`/`..` 边界、幂等性、
转义回环）+ 矩阵 68/68（minify 默认开）+ `--no-flatten` 全语料 +
同 seed 逐字节复现。两个语料真实暴露的坑：`- - 5`→`--5`（edge）、
`1 .. 2`→`1..2`（basics，Luau Malformed number）。

### ✅ M2 完成（2026-08-23）

| 模块 | 文件 | 说明 |
|---|---|---|
| L3 扁平化 | `luraph-rs/src/flatten.rs` | 函数级 CFG 状态机：块图（Join/Cond/Stmt/Return/循环节点）→ 随机状态 ID + 乱序 if/elseif 分派。**循环=嵌套子状态机**：循环体是独立分派，循环体局部变量（含 for 循环变量）声明在 per-pass 作用域（每轮重新 `local`）→ 闭包捕获循环变量每轮 fresh，与原生 for 语义一致。break/continue 为纯图边（continue→体尾：for/while=增量+重判条件=Luau 语义；repeat 的 until 检查块在内部分派内）。for-numeric 原生顺序：区间检查在体前（空区间 0 次迭代）、变量每轮 fresh、增量在体后。作用域安全：被跨分支/跨闭包/循环体引用的 local 提升到机器顶部（名字与全局名不相撞 → 无遮蔽） |
| L3 junk | `luraph-rs/src/junk.rs` | 无副作用算术垃圾块（只碰新局部变量）+ 恒真透明谓词 if；修复两个既有 bug：跨作用域引用 c、return 后注入 |
| desugar | `luraph-rs/src/desugar.rs` | **已退出流水线**（存档）：flatten 原生处理所有循环，无需 for→while 去糖（旧去糖还有 continue 语义缺陷：for 的 continue 会重跑循环变量赋值） |

**M2 验收**：矩阵 68/68 全绿（原 62 项 + `loops.lua` 共享语料 3 项 +
`luau_loops.lua` Luau 语料 2 项）；`--no-flatten` 路径全语料通过；
同 seed 逐字节一致/异 seed 编码不同；循环闭包捕获、continue、空区间
for、nested break、repeat+until 局部变量、全局同名遮蔽均有语料覆盖。

**M2 阶段修复的既有 bug**：
- 旧 flatten 的 `in_loop`（"能到达循环头"）误把**循环前的 init 块**
  （`local lim, stp, cur = ...`）当循环内局部变量，既不进 loop_hoisted
  又跳过机器顶提升 → 整体丢失 → 变全局引用（"attempt to perform
  arithmetic on global 'wgam'"）
- `junk.rs`：`local d = c + 1 - 1` 的 c 在 if 块内声明（跨作用域）；
  mid-body splice 会在 `return` 后注入语句（Lua 语法错误）
- 两者都被旧 flatten 的过度提升/死块丢弃行为**掩盖**，本次重写后暴露并修复

### ✅ M3-1: numbers 完成（2026-08-23，L4 数值）

| 模块 | 文件 | 说明 |
|---|---|---|
| L4 数值 | `luraph-rs/src/numbers.rs` | 数字字面量重写：整数 → 2~4 个带符号随机项之和/差（小值）或 `a*b+c` 积式（大值，c 小残差）；0/±1 保留；浮点仅恒等包裹（`0+x`/`x+0`/`x*1`/`x/1`，禁止拆分——改 IEEE 值）。状态机 ID、索引、运算常量全部覆盖。CLI：`--no-numbers` |

**numbers 验收**：单测 23/23（值精确、项非平凡、原值不可见、2000 种子
小值压力）+ 矩阵 68/68 全绿。顺带修复打印机 Idiv 去糖缺括号既有 bug
（`math.floor(l / r)` 操作数需 Bin 上下文括号，luau_idiv 语料暴露）。

### ✅ M3-2: body 完成（2026-08-23，L5 整体加密）

| 模块 | 文件 | 说明 |
|---|---|---|
| L5 整体加密 | `luraph-rs/src/body.rs` | 流水线最外层：打印当前程序（minify 紧凑形态）→ 全新 24B 密钥加性加密（复用 L2 密钥流，新密钥）→ 密文切 3~5 块 Str 字面量 → 复用 L2 加载器（密钥 3 段+字节表+DEC 解密函数）→ 最终 `loadstring(DEC(C1..CN))()`。输出只剩容器：无任何程序结构/明文，只有密文字符串 |

**body 验收**：矩阵 68/68 全绿（默认开，双解释器）；容器形态人工
确认（仅密钥+解密器+密文+一个 loadstring 调用）；loadstring 进保留名
（防遮蔽）。

### ✅ M3-3: antidbg 完成（2026-08-23，L7 反篡改 → M3 收官）

| 模块 | 文件 | 说明 |
|---|---|---|
| L7 反篡改 | `luraph-rs/src/antidbg.rs` | 容器层四重防御：① 金丝雀自检（纯函数 `((a*M)+C)%P`，执行前校验）② 容器校验和（密文字节和 mod 随机质数 vs 构建期期望值，解密前拦截）③ 错误重写（pcall 拦截 + 行号解析双方言格式 + 随机偏移重映射 + level 0 重抛）④ 时间陷阱（全程 os.clock，5~15s 随机阈值）。陷阱全部静默死循环（无消息/无退出码提示） |

**antidbg 验收**：矩阵 68/68 全绿（L1+L2+L3+L4+L5+L7 全开默认）；
专项：篡改密文字节→挂起 ✓、篡改金丝雀→挂起 ✓、未捕获错误行号
3→85852 重映射 + 无文件路径 + 退出码 1 ✓。

### ✅ M3 完成（2026-08-23）

L4 numbers + L5 body + L7 antidbg 三个 pass 全部落地，商业级预设的
非 VM 部分（L1+L2+L3+L4+L5+L7）已全量实现。

### ✅ M4 完成（2026-08-24）

**L6 VM 最小可用**：`vmgen/isa.rs`（41 条寄存器指令 + u16 定长编码 +
每构建 opcode 随机置换）+ `vmgen/compiler.rs`（AST→字节码：寄存器分配/
常量池/upvalue 活引用+快照双机制/多值调用协议 CallE/CallM/CallT/
repeat-until 作用域/循环捕获语义）+ `vmgen/template.rs`（解释器模板：
makefn 闭包帧模型 + 分派循环 + `__call`/`__len`/元表处理 + Luau 冻结
`_G` 规避 + 双方言运行期探测）。CLI `--vm` 开关；输出 = 混淆解释器 +
加密字节码。实现细节与全部语义坑：`docs/vm-l6-implementation.md`。

矩阵扩展为 **非 VM 102 项 + VM 102 项 = 204 项全绿**（语料 21→29：
新增 8 个 stress_* 应力用例；含 VM 输出在 luau 下的交叉验证 +
luau-compile 语法校验）。**多种子回归**（seeds 1/7/4242/31337/999999
× 全语料 × 双方言 × 双阶段 + 交叉）0 失败。种子确定性已验证
（同 seed 逐字节一致 / 异 seed 编码完全不同）。

修复（2026-08-24 后续）：**输出纯度 bug**——示例/输出中出现随机
繁体中文字符：密文串走打印器的 UTF-8 透传路径，随机高字节恰好构成
合法 UTF-8 时原样输出。修复 = `Expr::Str.is_binary` 全转义 + minify
字面量原样输出（细节见 `docs/vm-l6-implementation.md`）；全部 25 个
示例现均为纯 ASCII。

### ✅ M4 续期：VM 语义加固（2026-08-25）

**背景**：沙箱重置抹掉 `/home/user/tools` → 工具链整体重建进仓库
`luraph/.tools/bin`（Rust @rustbin / lua51 镜像源码 / Luau g++ 直编 +
自写 main 复刻 CLI 环境——**必须含 luaL_sandbox**，官方 luau CLI 全局表
只读，漏掉会让语料假通过）。

**语料扩容 21→29**（8 个 stress_*：upvalues/coroutines/metamethods/
multival/errors/bigtable/control/luau_vm），直接暴露并修复 **9 类真 bug**
（细节 `docs/vm-l6-implementation.md` §8）：

1. **upvalue 单 cell 别名模型**（换代）：materialize 从「GetUp 值副本 +
   写回转发」改为**纯作用域别名**（描述符 `0xC000|upidx`，闭包直接引用
   父帧 cell 对象）——精确 5.1 单 cell 语义，消灭跨层读写失同步
2. **循环变量/循环体局部 per-iteration 共享 cell**（`0x8000|slot`，
   V[slot] 存 `{1=value}` cell 表）：同迭代所有闭包 + 循环体共享，
   迭代间 fresh；未捕获的循环局部复用固定寄存器（无寄存器增长）
3. **CallT 表存储 off-by-one**（尾调用进表产生空洞）
4. **GetTab 索引错误缺失**（非 table 无 mt 应报错）+ **__index 表链**
   （rawget 断链 → 原生索引）
5. **Assign 尾部展开后的 nil 填充越界补写**（`k = next(t, k)` 必中）
6. **`return ...` / `a, b = ...` / `a, b = f()` 全变长展开** + 多余值
   求值即弃（保副作用）
7. **5.1 构造器存储序**（SETLIST 最后 → 重复键位置字段必胜；Luau
   源码序）——重复键用例因此不能进共享语料（原始程序双方言输出本就
   不同）
8. **打印机后缀括号**（`(5).nope` → `5.nope` 双方言皆非法 → Ctx::Suffix）
9. **parser 多目标赋值** `a, b = ...`；**Luau `for k,v in t` 隐式 next**
   在 parser 归一化为 `next, t`（5.1 档原样透传，裸 table 与宿主同错）

**验收**：矩阵 **204/204 全绿**；多种子回归（5 seeds × 29 语料 ×
双方言 × 双阶段 + 5.1→luau 交叉）**0 失败**；最小用例集逐项 diff 通过。

**环境级发现**（已写入 HANDOFF §4 + vm-l6 笔记 §8.3）：
- 官方 luau CLI **luaL_sandbox** 全局只读（0.600+）：顶层新建全局赋值
  = 报错。语料已清理（loops.lua shadowtest 改写）；VM SetGlobal 天然
  同错（语义镜像正确）。
- **for-in 方言差异**：Luau 隐式 next / 5.1 必须可调用。

### ✅ M5 完成（2026-08-25）：VM 完整随机面

| 项 | 落地 |
|---|---|
| SoA 平行数组 | `[ncode][W bytes][S0..S3 varint]`；pc 按指令步进；跳转 = 1 基指令下标 |
| 7-bit 完整档 | 7/14/21/28-bit + 128 进制 + 2³² 归一化；模板 `r16` 无位运算 |
| base-94 + token | 每构建字母表/保留前缀；10 个 5 字符 token（v15 pC 特殊字符集） |
| 解码枢纽/状态元组 | inline / `hub()` 两风格；6 元组序 + `run` 字段序 + helper 声明序 shuffle |
| 帧入场原语解包 | 15 原语 → `P[1..80]` 随机槽；VM 顶 + `run` 双解包 |
| 反编译抽查 | `luac51 -l` 仅见 L5 容器；用户串不可检索 |

**验收**：矩阵 204/204；多种子 0 失败；同 seed 一致 / 异 seed 前 2KB 重合 9.5%。
踩坑：SoA 数组不可与 opcode 名表同名（`OC`）；`AL`/`TK` 必须声明在 `decarrier` 之前。

### ✅ M6 完成（2026-08-25）：产品化

| 项 | 落地 |
|---|---|
| CLI 预设 | `--preset low\|medium\|high\|vm\|max`；默认 ≡ high；`vm` ≡ `--vm`；`max` = vm（v2 预留） |
| 产品文档 | 根目录 `README.md` 重写（用法 / 预设表 / 约束 / 验证） |
| 性能数据 | `docs/performance.md` + `tests/bench_presets.sh` |
| 预设矩阵 | `tests/run_presets.sh` **405/405** |

版本 0.1.0 → **0.2.0**。

### 🟡 v15 结构同族（路线 A）：P0 完成（2026-08-25）

| 项 | 落地 |
|---|---|
| 二轮详读样本 | 自研块级解析器全文复测；`docs/v15-structural-parity-plan.md` 订正 5 处架构误判（iL=mul32 非分发器 / 执行内联在 [18]/[73] / LCG 静态零调用=诱饵 / 真实密钥一次式 / cell `{[4]=bank,[7]=slot}`）+ 黄金数订正（227 字段 / 73 槽 / 28 字面量 / 3 行文件） |
| 新增指纹 | F21–F32：命名族、if 表达式、融合条件返回、复合赋值、参数遮蔽、模块表自变异、字节码自修改、诱饵代码、不透明算术、cell 布局、元组槽 makefn、初始化器族 |
| **路线拍板** | **✅ 路线 A**（Roblox/Luau 克隆档 `--preset v15`，弃 5.1；现有 `vm` 预设不动） |
| P0 脚手架 | `luraph-rs/tests/v15_fingerprint.py`：样本 **32/32 PASS**，现 `--preset vm` 产物 **1/32**（防假通过 ✓）；`--preset v15` CLI（Luau 门控、5.1 报错；P0 stub ≡ vm 管线） |

**P0 验收**：指纹脚本样本全绿 + 现产物全红 + CLI 开关存在；官方矩阵 204/204 + 预设矩阵 405/405 仍全绿。

### 🟡 v15 结构同族（路线 A）：P1 完成（2026-08-25）

外壳换代落地：`--preset v15` 产物 = **3 行**（头注释 + 空行 +
`return setmetatable({XX=function(b)<解释器>return function(...)return VM(<载体>)end end},{}):XX()(...);`，
boot 名每构建随机、裸标识符字段、结尾分号）。实现要点：解释器块先过
junk/mangle/numbers 再包壳（加载器进 boot、壳字段名不进 strings）；
v15 档关 strings/body/antidbg（F10/F2/F18 过渡形态，P4 blob 收回）。
**连带核心路径修复**：strings.rs 加载器引用改实名（空名+sym 绑定在
二次 resolve 下打印空名）+ symtab resolve 幂等。指纹 F1/F2/F18 PASS；
29 语料在 v15 档下全部输出一致；官方矩阵 204/204 + 预设 405/405 全绿。
**下一步**：P2 数字槽原语 + 入场解包 + 命名方案（字段量级冲样本 227）。

### 🟡 v15 结构同族（路线 A）：P2 完成（2026-08-25）

模块表铺开：`vmgen/v15.rs` 落地——**65 原语数字槽**（槽号 1..126 稀疏
洗牌；Vector3/Vector2 为 Roblox 专属、CLI 缺失，不发射）+ **2 诱饵 LCG
工厂槽**（状态机化、参数遮蔽、静态零调用）+ 1 可变常量槽 + 10 具名常量
字段（AL/BL/…/h 死表），全字段洗序混存。指纹 **F9（69 槽/66 原语槽）、
F15、F17、F19、F25、F28、F29 PASS** + F1/F2/F18 保持（共 10/32；其余
量级指标属 P3+）。boot 名改字母表族（基字母+`C` 后缀，样本 `FC` 形态）。
验证：29 语料 × v15 档一致；多种子（5 seeds × 4 语料）0 失败；官方矩阵
204/204 + 预设矩阵 405/405 全绿。
**下一步**：P3 引导管线 handler 化 + 执行循环内联化（最重阶段），
入场原语解包并入帧运行器重写。

### 🟡 v15 结构同族（路线 A）：P3 增量 1 完成（2026-08-25）

CPS 引导骨架落地（`vmgen/v15.rs::scaffold`）：boot 换代为样本形态的
**初始化器（`return true,<s0>,nil×10`）+ 顶层机（FC 形态：while 旗标 +
控制码协议）+ CPS 循环（二叉范围树 + `continue` 叶，全集中单函数）+
单字符 handler 链（每原型 1 个 staging handler：载体入上下文表 + 校验和
经 mul32/mod2³² 算术助手折叠，真实调用非死码）+ 解释器定义 handler
（混淆管线产物以文本嵌入）+ 控制码 handler（2=返回/1=继续）**。
状态池每构建随机（3..236）、元组填充序每 handler 洗牌、脚手架名走
53 字母表族（≤2 字符）。解释器本体仍过 junk/mangle（strings/numbers
对 v15 关：字面量形态 + 状态 ID 裸值）。**指纹 10→11/32**：`functions`
语料 F3=103 字段、F4=24 具名函数、F5=20 returns/19 IDs/55 方法调用、
F7=18 continue——**指标随原型数线性扩展**。验证：29 语料 + 多种子
5×6 全一致；官方矩阵 204/204 + 预设 405/405 全绿。
**下一步**：P3-A 扩容（解码管线细粒度拆分冲样本量级）+ P3-B 执行内联。

### 🟡 v15 结构同族（路线 A）：P3 增量 2 完成（2026-08-27）

staging 细粒度拆分 + 校验阶段：每原型 = **字节和折叠 handler + 3 分块
存储 handler**（入口构造期拼回），**定义 / 入口构造拆成两个 handler**，
新增**校验阶段**（v1 对存储分块按字节和重折叠，v2 比对——不匹配进
静默死循环；实测篡改 1 字节即挂起，无消息无退出码，不碰 os.clock，
F18 安全）。`functions` 语料实测：**157 字段 / 78 具名函数 / 74 returns /
73 状态 IDs / 143 方法调用 / 72 continue**，指纹 **12/32**（增量 1 为
103/24/20/19/55/18，11/32）。验证：30 语料 + 多种子 5×6 一致；官方矩阵
204/204 + 预设 405/405 全绿；全部 30 个 v15 示例重新生成 + 语法检测通过。
**下一步**：P3-B 执行内联（双运行器多段内联树 + 自修改 + 死路径）。

### 🟡 v15 结构同族（路线 A）：P3-B 增量 3 完成（2026-08-28）

执行侧安全三件套：
1. **Nop 别名自修改**（`isa.rs` + `template.rs`）：OpMap 派生第二个 Nop
   wire 值（确定性选取，不耗 rng——**legacy 预设输出逐字节零变化**，已验证）；
   分发树双 Nop 叶；加载期把偶数下标 Nop 位改写为别名——字节码装载时
   自变异，opcode 编码流内不稳定，静态分析无法锁定单一映射。
2. **死循环段**：site1 形态的永不到达 fetch 树（while +1，F20 PASS）。
3. **双数字槽运行器**（`v15.rs::scaffold`）：解释器本体迁入 `[r1]`
   （样本 [73] 形态，索引调用显式传 self），用户入口经 `[r2]`（样本 [18]
   形态）路由——调用路径跨两个数字槽函数；模块表函数槽 = 4（样本同数）。

`functions` 语料：158 字段 / 87 `=function(` / 71 数字槽（样本 73）/
8 while，指纹 12 → **13/32**。验证：30 语料 + 多种子 5×6 一致；矩阵
204/204 + 预设 405/405 全绿；30 个 v15 示例再生成 + 语法全过；
**非 v15 示例逐字节零变化**。
**下一步**：执行循环真正内联进运行器 + 字面量自修改（F14/F27）+ 分号发射（F11）。

### 🟡 v15 结构同族（路线 A）：P3-B 增量 4 完成（2026-08-28）

字面量自修改 + 分号发射 + v15 命名族：
1. **编译器传 Nop 位置表**（`VmProgram.nop_sites` 与 `fns` 平行，
   `finish()` 定型期采集），模板发射 `local <W>=PF[k].W; <W>[p]=<别名>`
   **定值写回**——样本 `J[Q]=12` 形态落地，F14=76/F27=19 PASS。
2. **打印机语句间发 `;`**（minify 保留）——F11 `...;if` 形态基础；
   全部预设输出形态变更（语义矩阵验证）。
3. **v15 命名族**：mangle 加 `v15` 模式（1–2 字符/53 字母表/80/20）；
   脚手架保留名进 reserved 防解释器局部遮蔽。
4. **v15 关 numbers**（实测踩坑：数值拆分会改写自修改常量）。

指纹 13 → **14/32**；F11/F14 正则对齐 ≤2 字符命名族（样本单字符 32/32 保持）。
验证：30 语料 + 多种子 5×6 一致；矩阵 204/204 + 预设 405/405 全绿；
30 个 v15 示例再生成 + 语法全过。
**下一步**：执行循环真正内联进运行器（F11 fetch 形态随之扩量）。

### 🟡 v15 结构同族（路线 A）：P3-A 增量 5 完成（2026-08-28）

顶层机族扩容（计划 P3-A.3 落地）：
1. **子分发器层**：FC 与 CPS 循环之间插入 `XL`（样本第二分发层形态：
   while 旗标 + 范围树，两区间均汇入状态驱动的 CPS 循环——路由任意
   状态都正确）。调用链 = 顶层机 → 子分发器 → CPS 循环 → handler 链，
   与样本三层分发同构。
2. **2 个附加机初始化器**（样本 `KL`/`M`/`zL` 机族形态，起始 ID 在分发
   区间外）——F32 转 PASS。

指纹 14 → **15/32**（F4=90、F20=9 while、F32=3）。验证：30 语料 + 多种子
5×6 一致；矩阵 204/204 + 预设 405/405 全绿；30 个 v15 示例再生成 + 语法全过。
**下一步**：执行循环真正内联进运行器（F11/F12/F5 随之扩量）+ P4 blob。

### 🟡 v15 结构同族（路线 A）：P4 增量 6 完成（2026-08-28）

载体密钥流混淆（P4 安全内核）：
- 每个载体分块构建期用**位置相关密钥流 `(K1*i+K2)%256` + bxor** 加密
  （K1/K2 每构建随机），分块 handler 运行时经 `bit32.bxor` 解回。
  **存储字面量不再是可直接 base-94 解码的明文**——攻击者必须先还原
  密钥流（藏在混淆代码里）。XOR 往返精确（30 语料 + 多种子 5×6 验证）。
- 指纹 15 → **16/32**（F16 PASS：%256=55、bxor=52）。
- **架构注记（F8 张力）**：样本长串形态要求可打印内容，而 XOR 后的
  任意字节经我方输出管线（Rust String → UTF-8 → 解析器）只能用 `\ddd`
  转义短串承载——长串与 XOR 互斥。本档优先安全；长串形态需打印器
  直出字节流，列为后续独立改造。

验证：30 语料 + 多种子一致；矩阵 204/204 + 预设 405/405 全绿；
30 个 v15 示例再生成 + 语法全过。
**下一步**：执行循环真正内联进运行器（F11/F12/F5 大头）。

### 🟡 v15 结构同族（路线 A）：安全增量 S1 完成（2026-08-28）

**用户优先级明确：安全 > 结构。** 按真实安全差距补齐：
- **S1 密钥流状态机化**（消灭最高危差距）：增量 6 的载体 XOR 曾用
  `(K1*i+K2)%256`、K1/K2 明文常量写在每个分块 handler（攻击者找到一个
  即读出密钥）。现改为样本 `[96]` 同款——**KSC 槽**（LCG 状态，播种）+
  **KG 槽**（3 步 LCG 状态机生成器，常数内嵌状态转移体）；分块 handler
  运行时调 `b[KG](b)` 取密钥，**解码点无任何密钥常量**，静态不可提取。
  构建期同一套 3 步 LCG 生成密钥流，往返一致。
- 安全对账：字节码虚拟化/每构建唯一/自修改/反篡改 我们 ≥ 样本；密钥流
  差距已修；剩操作数不透明度（S2）+ 动态分析阻力（阶段 C）。

验证：30 语料 + 多种子 5×8 零失败；矩阵 204/204 + 预设 405/405 全绿；
非 v15 示例 0 变更；`268435456` 仅在 KG 生成器（1 处）。
**下一步**：继续结构阶段 C（执行 CPS 化，安全+结构双收益）或安全项 S2
（操作数 7-bit 分块不透明化）。

### 📄 Luraph v15 防御手段全解析（2026-08-28）

应用户要求对样本做**防御导向复测**（区别于 `luraph15-analysis.md` 的结构
导向），产出 `docs/luraph15-defense-analysis.md`：
- **19 项防御手段逐条取证**（全部数字本次脚本实测）：数据层（blob 74 566B /
  pC 10 条 / 全文件仅 30 字面量 / buffer 17 处）、密码（33 次 LCG 转移 /
  68 次状态槽写 / 91 次 bxor / 密钥常数 18/201/160）、控制流（146 函数 /
  351 状态返回 / iL 330+vL 239 热调用）、指令（264 次 -128 偏置 / 8 处
  自修改 / ~125 超级指令）、执行（77 数字槽 / CPS 帧）、错误重写（nL）
- **弱点诚实记录**：`os.clock`/校验和/`loadstring` 均 **0 处**（无时间陷阱、
  无显式完整性——样本软肋、我们的差异化空间）
- **采纳映射表**：19 项 → 我们的现状/决策（已吸收/计划中/拒绝）+ 最该补的
  安全差距排序（S2 操作数分块 → 阶段 C/D CPS → P4 blob）

### ⚠️ v15 结构同族（路线 A）：阶段 C 首尝试——发现栈代价，已回滚（2026-08-28）

执行循环 CPS 化（每指令独立 handler 分派）首次实施：14/15 针对语料过，
但 **`tail(5000)` 深尾递归栈溢出**。根因 = CPS 每指令分发一次函数调用，
函数调用递归进 `run` 时每层多一个栈帧；我们的尾调用是 `lastn/lastbase`
模拟（不做真 TCO），深尾递归再叠加一帧即爆。**属架构固有栈代价，非
bug**。已按回滚门回滚，恢复内联分发，全量回归回到 204/204 + 405/405
全绿。详见 `docs/v15-pipeline-rewrite.md` 阶段 C 注记。
**待决策**：栈代价解法二选一——① 内联 Call 系 opcode（中复杂度，破
handler 纯度）② 真尾调用优化/帧复用（根治，但调用约定根本改动、风险
最高）。选定后再推进阶段 C。

### ✅ v15 结构同族（路线 A）：阶段 C 第一步——真 TCO 落地（2026-08-28）

用户选定方案 ②（真尾调用优化）。实现方式：不靠注册表识别 VM 闭包，
而是让 `makefn` 闭包对 `run` 的调用成为**真尾调用**，由 Lua 原生 TCO
复用栈帧：`Return` 改返回 `U(out,1,total)`（解包多值），闭包/顶层调用
改 `return run(...)`。深尾递归不再每层叠 `run` 帧——`tail(5000)` 实测
通过（此前 CPS 版溢出）。验证：30 语料 + 多 seed 5×8 + 矩阵 204/204 +
预设 405/405 全绿；3 个 `.vm` 样本因共享 `run` 字节级变化（语义通过）。
**这是阶段 C 的地基**：CPS 分发将在此地基上重做。

### ✅ v15 结构同族（路线 A）：阶段 C 完成——执行 CPS 化（2026-08-28）

在真 TCO 地基上重做 CPS 分发并解决栈代价：
1. **真 TCO**（第一步）：`makefn` 闭包对 `run` 真尾调用，Lua 原生 TCO
   复用栈帧（`Return` 改返回解包多值）。
2. **CPS 分发**（第二步）：`run` 分发改为每指令一个 `H[OC.*]` handler；
   `Return` handler 返回信号 `{out, total}`，循环侧尾调用解包。
3. **内联 Call 系**（关键）：实测真 TCO 只省「闭包→run」层，CPS 经
   `H[Call]()` 仍每调用叠帧、`tail(5000)` 仍溢栈；把 Call/CallE/CallM/
   CallT 内联进 CPS 循环（不走 `H[]`）后递归不再叠该帧 → **深尾递归
   不溢栈**。

验证：`tail(5000)` 通过；30 语料 + 多 seed 5×8 + 矩阵 204/204 +
预设 405/405 全绿；30 个 v15 示例再生成 + 语法全过；非 v15 示例 0 变更。
指纹 F4 90→133（H 表 42 handler）。
**剩余**：F11（多段 fetch 循环）/F5（状态返回量级）等指纹收尾打磨，
属结构同族的细化而非新风险。

### 🟡 虚拟机建议全量落地（2026-09-05 起，用户 main 分支「虚拟机建议.md」）

用户要求 5 条建议全做 + 未完全契合处重做，最终完全按该方法生产混淆。
**增量 ① 建议3 ✅：load/loadstring hook 完整性检测**——guard 新增
`isNative`（`debug.info(f,"s")=="[C]"`）：`loadstring`/`load` 存在但
非原生（被 Lua 包装 hook）→ failed → 静默挂起。Luau 下 `load` 为 nil
自动跳过不误报；5.1 无 `debug.info` 自动降级跳过。验证：原生
`loadstring` 通过、Lua 包装被识别（逻辑直测）+ 204/405 全绿 + v15
抽查 5/5（运行一致 + 32/32 保持）。
**待做**：建议4（字节码高熵序列化）→ 建议5（bit 存储，Luau）→
建议2（anti/ 文件夹 + 真假分支 + 指令级环境检测）→ 建议1（指令拆分
多形态）。

### ✅ v15 反调试 guard（CHAR 编码版，2026-08-29）

用户要求 v15 也带 guard；字面量会破 F10，故 `v15_guard_source()` 把
guard 全部字符串（11 个，去重）改 `GS[k]=schar(码…)` 数值重建，注入
**FC 入口机头部**（顶层 3 行不变）；string 被 hook 时 GS 为空 → 比较
自然失配触发 abort。验证：30 语料×5 种子 150/150 + 指纹 30/30 个
32/32 保持 + 204/405 + 多 seed + cargo test 27；print6 v15 带 guard
重生上传。标准管线仍用 mangle 前奏版；`--no-guard` 两路同关。

### ✅ 新防护层：反调试环境完整性 guard（2026-08-29，用户提供设计）

`guard.rs`：自包含 IIFE 注入标准管线输出载荷之前（默认开，
`--no-guard` 关）。环境检查：核心全局为真函数 / getfenv 健全 / 负键
读写回环 / `pcall(error)` 必须失败 / `debug.info` 行号探针（自报行号
= 错误消息行号，`error` 的 source 必须 `[C]`）/ newproxy + 表 canary
（`__tostring/__concat/__call/__iter` 绊线）/ `unpack({},0,64)` /
env print/warn 同一性。失败 → `_ENV` 污染循环静默挂起（按项目约定
移除了原稿的调试打印）。注入前 parse→mangle→minify，内部名构建随机。
**v15 版同日跟进**：字面量会破 F10≤60，故改 CHAR 编码注入 FC 入口机
（见上一条「v15 反调试 guard」），32/32 指纹零破坏。
验证：204 + 405 + 多 seed 全绿（零误报）；负测 5.1 劫持 `pcall`/
`error` → 124 挂起、载荷不执行；示例重生后 v15 逐字节不变、纯度保持。

### ✅ v15 结构 100% 还原战役（2026-08-29，用户拍板：先结构后安全）

**增量 E1 ✅（指纹 17→27/32）**：复合赋值折叠（打印器 `x=x+1`→`x+=1`，
纯左值门控）+ CHUNKS 3→5 扩容 + 融合条件诱饵局部（F23）+ fetch 形扫描
循环（F11）+ `=N,function(...)` 包装（F12）+ 高位诱饵 LCG 槽（F28）+
元组槽 `b[J[4]]/b[K[7]]` 路由（F31/F30）+ 命名字段写（F26）。
全量回归绿（30 语料 + 204 + 405 + 多 seed），示例纯度保持。

**增量 E2 ✅（指纹 27→29/32）**：① F6 宽参数（1→109）——状态元组
3→5 填充槽，staging handler 全部 7 参，CPS 叶/循环/顶机穿透 `f1..f5`；
② F22 if 表达式（0→18）——解析器实现 Luau if-expr，`Expr::IfExpr`
打通 clone/symtab/numbers/打印器/VM 编译器（条件级联 Jf+Jmp），fold
诱饵与控制 handler 用 `=if` 形态；用户源码 if 表达式双档语义一致。
**增量 E3 ✅（指纹 29→30/32）**：① F13 内联 -128 阶梯（6,6→54,54）——
4 条操作数流改每流内联四级阶梯 + 校验重读（9 份阶梯）；② 真操作数
完整性校验——编译器折叠每原型线操作数和（`operand_sums`），解码环比对
`ck` 不等即静默陷阱。
**增量 E4 ✅（F8）**：RC 载体长串（≥10.5KB `[[...]]`，管道级安全级别
自动选级）+ RT 反查 token 表（pC 形 1 字符键→5 字符值 ×10）；XOR 分块
改存 HB 十六进制长串（dehex handler 运行时还原），chunk 短字面量清零；
verify 对 RC 切片做逐字节相等真校验。
**增量 E5 ✅（F10 233→33）**：解释器字符串池 `StrPool`——元方法/类型/
错误消息/`'#'` 全部走启动期 `MS` 表（`CHAR(码…)` 数值构建）；TK 表改
`CHAR` 数值构建；chunk 字面量随 E4 清零；最终可见短串 ≈33（RT 20 +
具名常量 + 路由串），≤60 达标。
**规模地板 + 碰撞修复**：staging 链填充至 ≥100 handler（小源码也保
F4/F5/F6/F21/F23 量级）；诱饵槽 `decoy_slots(max_carrier_len,avoid)`
置于「最大载体长度+500」之上，避开模块槽/跑者/原语/AL 字节键/Nop
自修改字面位置（多轮种子回归暴露的三类碰撞逐一修复）；RC 重编码校验
改直接相等（原按单字符过 RT 在 reserved∈specials 时种子相关触发陷阱）。
**长串 5.1 兼容修复**：打印器长串选级补「内容含裸 `[[` 时级别 ≥1」
（Lua 5.1 拒绝 level-0 嵌套 `[[`，strings/5.1 回归修复）。

**战役结果 ✅：32/32 指纹 × 全部 30 语料 × 5 种子（150/150 运行时 +
30/30 指纹），矩阵 204/204 + 预设 405/405 + 多 seed + cargo test 27
全绿，示例纯度 0 非 ASCII。结构 100% 还原达成。**
**收官后修正（用户报告）**：① RC 填充常量 `'a'` 产生显眼 `aaaa…` 长串
→ 先改随机字母表字节，后发现**明文载体长串本身**会把字节码 0 字节段
暴露为同字符长串（样本最长重复仅 8）→ **删除明文 RC 长串**：F8 长串
职责全归 HB（XOR 后十六进制，天然无结构），随机十六进制填充到 ≥10.5KB；
完整性由既有双重折叠保证（staging 折叠 vs verify 折叠，v2 比对，不等
即陷阱）。修正后全示例最长同字符重复 ≤7（与样本同量级），回归全绿。
**下一步**：按用户路线回到安全增强（阶段 D 每帧闭包/动态分析阻力等）。

### ✅ bug 修复：v15 输出混入繁体中文/特殊字符（2026-08-29）

现象：所有 `*.v15.luau.lua` 示例含数百个非 ASCII 字符（`˰ƨ즀ી` 等）。
根因：词法器解析字符串一律 `is_binary=false`；v15 载体 blob 以 `\ddd`
转义字面量注入后经 `parse_expr` 重词法、重打印时，打印器 UTF-8 直通
分支把恰好构成合法 UTF-8 的密文字节还原成字符。修复：词法器为
「转义产生 ≥0x80 字节」的字符串打 `high_bytes` 标志（短字符串 +
反引号插值两路径），解析器升格 `is_binary`，打印器对高字节全转义；
源码原生 UTF-8 不受影响。验证：64 示例 0 非 ASCII + 30 语料 + 矩阵
204/204 + 预设 405/405 + 多 seed 全绿；指纹保持 17/32。

### ✅ v15 结构同族（路线 A）：阶段 A 完成——操作数散布（2026-08-28）

用户选定选项 A（唯一仍有真实安全价值的剩余项）。操作数散布落地
（样本 §10.4 / 防御 D1 家族）：
1. **寄存器散布**：每函数逻辑寄存器 → 随机物理槽 σ（~50% 密度，
   大帧封顶 +64）。单寄存器操作数直接写散布值；LoadNil/Call 系/Return
   等范围型操作保留逻辑基址，运行期经 blob 内**槽表 S** 翻译。
2. **常量散布**：常量入更大常量池随机位置（空洞 nil/诱饵数字填充）。
3. **upvalue 置换**：upsrc 每函数随机置换；父帧寄存器引用在 `makefn`
   里经**父帧的 S** 翻译（子函数先于父函数序列化也不冲突）。
4. **溢出表 O**（关键修复）：`nres=255` 调用返回值可超过寄存器分配数
   ——散布后固定 S 会越界（`table index is nil`），Call/CallE/CallM
   无界写 + Return lastbase 读 + 尾部展开值读全部带 `S 缺位 → O` 回退。

效果：寄存器/常量/upvalue 操作数共享同一不透明数字空间，静态寄存器
恢复失去小整数锚点；σ/κ/τ/S 全部在载体编码 blob 内，输出不可见。
验证：30 语料 + 多 seed 5×8 + 矩阵 204/204 + 预设 405/405 全绿；
`tail(5000)` 通过；非 v15 档 0 变更；新增单测
`scatter_layout_properties`（槽唯一性/范围/散布 + 线操作数越密集区）。
**剩余**：阶段 D（每帧闭包，高风险）/阶段 E（指纹量级收口）——均为
结构同族收尾打磨，无新的安全差距。

### ⬜ 剩余

无未完成里程碑。可选增强见 `docs/implementation-plan.md` §1 仍为 📐/⬜ 的行。
每个 pass 完成后仍执行强制工作流。

## 4. 目录结构（当前，2026-08-25）

```
luraph/
├── HANDOFF.md                  # 零上下文交接文档（新会话先读）
├── PROGRESS.md                 # 本文件
├── README.md                   # 产品 README（2026-08-25 重写）
├── .tools/                     # ★ 工具链（gitignored，708M；重建步骤 HANDOFF §4）
│   └── bin/{rustc,cargo,lua51,luac51,luau,luau-compile}
├── docs/
│   ├── obfuscation-research.md # 混淆技术学习记录（VM 设计草案 + 实现规划 + 语料清单）
│   ├── luraph15-analysis.md    # Luraph v15 样本分析报告（含第二轮深挖 15 项新机制
│   │                           #   + 25 项功能对比表 + 分析边界声明）
│   ├── implementation-plan.md  # ★ 实施计划：将实施哪些混淆方法（L1–L7 逐项）+ 目前进度
│   │                           #   + 里程碑 M0–M6 验收标准（活文档）
│   └── vm-l6-implementation.md # ★★ VM 实现笔记（架构/多值协议/upvalue 单 cell 模型
│                               #   /9 类 bug/环境语义/M5 清单/调试工具箱）—— 动 VM 前必读
├── samples/
│   ├── luraph15.txt            # 用户提供的 Luraph v15.0 混淆样本（171KB）
│   ├── luraph15.lua            # 可执行工作副本（Vector2/3 内联替换）
│   ├── luraph15_trace.lua      # 动态分析副本（5 分发点 opcode 探针 + 看门狗，生成物）
│   ├── make_trace.py           # 探针注入生成器
│   ├── run_trace.lua / run1.lua# 动态运行包装器
│   └── polyfill.lua            # buffer/Vector3 polyfill（buffer 部分因 CLI 内建而未用上）
├── luraph-rs/                  # ★ Rust 混淆器（M5 收官，矩阵 204/204 全绿）
│   ├── Cargo.toml              #   std-only 零依赖
│   ├── src/
│   │   ├── main.rs             #   CLI（--dialect/-o/--seed/--vm +
│   │   │                       #     --no-mangle/--no-strings/--no-flatten/--no-junk/
│   │   │                       #     --minify(默认)/--no-minify/--no-numbers/
│   │   │                       #     --no-body/--no-antidbg）+ 管线装配
│   │   ├── ast.rs / lexer.rs / parser.rs / symtab.rs / printer.rs   # M0 地基
│   │   │                       #   （printer Ctx::Suffix = 后缀位置括号规则）
│   │   ├── mangle.rs / minify.rs / strings.rs      # L1 名称 / L1 压缩 / L2 字符串
│   │   ├── flatten.rs / junk.rs                    # L3（flatten 内含循环降级 make_loop）
│   │   ├── numbers.rs / body.rs / antidbg.rs       # L4 数值 / L5 整体加密 / L7 反篡改
│   │   ├── rng.rs / rng_check.rs                   # 种子 PRNG
│   │   ├── desugar.rs            #   ⚠️ 孤儿文件（未挂 mod，死代码存档）
│   │   └── vmgen/                # ★ L6 VM
│   │       ├── isa.rs            #     41 条 Op + SoA + 完整 7-bit + Carrier
    │   │       ├── compiler.rs       #     AST→字节码（单 cell + 指令下标跳转）
    │   │       ├── template.rs       #     解释器（SoA/decarrier/hub/原语解包）
│   │       └── mod.rs
│   ├── tests/
│   │   ├── cases/*.lua         #   ★ 29 个测试语料（20 共享 + 9 luau_*；8 个 stress_*）
│   │   ├── run_tests.sh        #   ★ 官方矩阵（204 项：非 VM 102 + VM 102，含交叉）
│   │   ├── multiseed.sh        #   ★ 多种子回归（VM 改动必跑，seeds 可传参）
│   │   └── gen_examples.sh     #   生成混淆示例
│   └── tools/
│       └── luau-cli-mains/     #   重建 .tools 的 luau/luau-compile 自写 main（权威副本）
├── examples/（在 luraph-rs/examples/）
│   └── *.5.1.lua / *.luau.lua / *.vm.5.1.lua
│                             # ★ 所有常用语法的混淆示例（对照 tests/cases/）
└── lph/                        # 早期 Lua 参考实现（已被 Rust 取代，仅存档）
    ├── rng.lua
    ├── lexer.lua
    └── parser.lua
```
