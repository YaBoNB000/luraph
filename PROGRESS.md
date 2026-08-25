# 项目进度（PROGRESS）

> 最后更新：2026-08-25
> 当前状态：✅ 环境 ✅ 研究 ✅ v15 分析 ✅ M0 地基 ✅ M1 词法+字符串 ✅ M2 控制流
> ✅ M3 数值+整体加密+反篡改 ✅ **M4 VM 完成 + 续期加固（2026-08-25）**
> （L6 私有字节码 VM：41 条寄存器指令 + 每构建 opcode 随机置换 + 生成的混淆解释器；
> 语料 21→29（新增 8 个 stress_*）× 双方言 ×（lua51 + luau 交叉）矩阵 **204 项全绿** +
> **多种子回归**（seeds 1/7/4242/31337/999999）0 失败；upvalue 单 cell 别名模型 /
> 循环变量 per-iteration 共享 cell / 5.1 构造器存储序 / 全变长展开等 9 类语义坑
> 踩平并记录于 `docs/vm-l6-implementation.md` §8；工具链已重建进仓库
> `luraph/.tools/bin` 抗沙箱重置；M5 随机面已落地约 60%，剩 SoA/base-N/枢纽 ID/入场解包）

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

### ⬜ 剩余（按 implementation-plan.md 里程碑推进）

- [ ] **M5**（~60% 已落地）：**VM 完整随机面**。已落地：随机决策树分派
      （2~4 层）/ Nop 死指令填充 / 7-bit 变长基础档 / slot_perm 操作数
      槽随机。剩：SoA 平行数组容器、7-bit 完整档（7/14/21-bit）、
      解码枢纽/状态元组位置随机化、base-N 编码 + token 转义、
      帧运行器入场原语解包随机化、反编译人工抽查
      （见 `docs/vm-l6-implementation.md` §7）
- [ ] **M6**：CLI 预设（low/medium/high/vm）+ README 产品文档
- 每个 pass 完成后执行强制工作流（矩阵全绿 + examples 重新生成才算完成）

## 4. 目录结构（当前）

```
luraph/
├── HANDOFF.md                  # 零上下文交接文档（新会话先读）
├── PROGRESS.md                 # 本文件
├── README.md
├── docs/
│   ├── obfuscation-research.md # 混淆技术学习记录（VM 设计草案 + 实现规划 + 语料清单）
│   ├── luraph15-analysis.md    # Luraph v15 样本分析报告（含第二轮深挖 15 项新机制
│   │                           #   + 25 项功能对比表 + 分析边界声明）
│   └── implementation-plan.md  # ★ 实施计划：将实施哪些混淆方法（L1–L7 逐项）+ 目前进度
│                               #   + 里程碑 M0–M6 验收标准（活文档）
├── samples/
│   ├── luraph15.txt            # 用户提供的 Luraph v15.0 混淆样本（171KB）
│   ├── luraph15.lua            # 可执行工作副本（Vector2/3 内联替换）
│   ├── luraph15_trace.lua      # 动态分析副本（5 分发点 opcode 探针 + 看门狗，生成物）
│   ├── make_trace.py           # 探针注入生成器
│   ├── run_trace.lua / run1.lua# 动态运行包装器
│   └── polyfill.lua            # buffer/Vector3 polyfill（buffer 部分因 CLI 内建而未用上）
├── luraph-rs/                  # ★ Rust 混淆器（M2 完成，矩阵 68/68 全绿）
│   ├── Cargo.toml              #   std-only 零依赖
│   ├── src/
│   │   ├── main.rs             #   CLI（--dialect/-o/--seed/--no-mangle/
│   │   │                       #     --no-strings/--no-flatten/--no-junk/
│   │   │                       #     --minify(默认)/--no-minify/--no-numbers/
│   │   │                       #     --no-body/--no-antidbg)
│   │   ├── ast.rs              #   AST 定义
│   │   ├── lexer.rs            #   双方言词法（11 单测）
│   │   ├── parser.rs           #   双方言语法（Pratt/注解剥离/去糖）
│   │   ├── symtab.rs           #   作用域解析
│   │   ├── printer.rs          #   打印器（优先级括号/字节精确字符串）
│   │   ├── mangle.rs             #   L1 名称混淆（保留 self：方法固定参数名）
│   │   ├── minify.rs             #   L1 token 感知单行压缩（默认开）
│   │   ├── strings.rs            #   L2 字符串加密 + 运行时解密加载器
│   │   ├── numbers.rs            #   L4 数字字面量拆分（默认开）
│   │   ├── body.rs               #   L5 整体加密容器（默认开）
│   │   ├── antidbg.rs            #   L7 反篡改容器层（金丝雀/校验和/错误重写/时间陷阱）
│   │   ├── junk.rs               #   L3 垃圾代码 + 透明谓词
│   │   ├── flatten.rs            #   ★ L3 CFG 扁平化状态机（循环=嵌套子状态机）
│   │   ├── desugar.rs            #   （已退出流水线，存档）
│   │   ├── rng.rs              #   Park-Miller PRNG
│   └── tests/
│       ├── cases/*.lua         #   21 个测试语料（含 loops/luau_loops）
│       ├── run_tests.sh        #   测试矩阵（68 项检查）
│       └── gen_examples.sh     #   生成混淆示例
├── examples/（在 luraph-rs/examples/）
│   └── *.5.1.lua / *.luau.lua  # ★ 所有常用语法的混淆示例（含 L3 扁平化）（对照 tests/cases/）
└── lph/                        # 早期 Lua 参考实现（已被 Rust 取代，仅存档）
    ├── rng.lua
    ├── lexer.lua
    └── parser.lua
```
