# 项目进度（PROGRESS）

> 最后更新：2026-08-23
> 当前状态：✅ 环境 ✅ 研究 ✅ v15 分析 ✅ M0 地基 ✅ M1 词法+字符串 ✅ **M2 控制流完成**
> （L3 CFG 扁平化状态机 + 循环嵌套子状态机 + junk；L1 minify 单行压缩已补齐；
> 全语料混淆示例见 `luraph-rs/examples/`（现为单行紧凑形态）；矩阵 68 项全绿；
> 下一步 M3 = L4 数值 + L5 整体加密 + L7 反篡改）

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
| Lua 5.1 解释器 | 5.1.5（源码编译） | `/home/user/tools/bin/lua51` | 验证 5.1 目标输出正确性 |
| luac 5.1 | 5.1.5（源码编译） | `/home/user/tools/bin/luac51` | 5.1 字节码编译检查 |
| Luau 解释器 | 0.735（源码编译） | `/home/user/tools/bin/luau` | 验证 Luau 目标输出正确性 |
| Luau 编译器 | 0.735（源码编译） | `/home/user/tools/bin/luau-compile` | Luau 输出语法校验 |
| Luau 分析器 | 0.735（源码编译） | `/home/user/tools/bin/luau-analyze` | Luau 静态检查 |
| Rust 编译器 | 1.88.0 stable | `/home/user/tools/rust-1.88/`（`/home/user/tools/bin/rustc`） | 混淆器本体编译 |
| Cargo | 1.88.0 | `/home/user/tools/rust-1.88/cargo`（`/home/user/tools/bin/cargo`） | Rust 构建 |
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

### ⬜ 剩余（按 implementation-plan.md 里程碑推进）

- [ ] **M3**：`numbers`(L4) + `body`(L5 整体加密) + `antidbg`(L7)
- [ ] **M4/M5**：**VM（L6）**：`vmgen/isa.rs` + `compiler.rs` + `template.rs` + VMC 每构建随机化
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
│   │   │                       #     --minify(默认)/--no-minify）
│   │   ├── ast.rs              #   AST 定义
│   │   ├── lexer.rs            #   双方言词法（11 单测）
│   │   ├── parser.rs           #   双方言语法（Pratt/注解剥离/去糖）
│   │   ├── symtab.rs           #   作用域解析
│   │   ├── printer.rs          #   打印器（优先级括号/字节精确字符串）
│   │   ├── mangle.rs             #   L1 名称混淆（保留 self：方法固定参数名）
│   │   ├── minify.rs             #   L1 token 感知单行压缩（默认开）
│   │   ├── strings.rs            #   L2 字符串加密 + 运行时解密加载器
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
