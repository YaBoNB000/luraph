# 项目进度（PROGRESS）

> 最后更新：2026-08-23
> 当前状态：✅ 环境 ✅ 研究 ✅ v15 分析 🟢 **M0 地基完成**（用户已发「开始」信号；
> Rust 骨架 + lexer/parser/symtab/printer 完成，round-trip 62 项检查全绿）

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

### ⬜ 剩余（按 implementation-plan.md 里程碑推进）

- [ ] **M1**：`mangle`(L1 名称混淆) + `strings`(L2 字符串加密 + 运行时加载器)
- [ ] **M2**：`desugar`/`flatten`/`junk`(L3 控制流：CFG 扁平化状态机)
- [ ] **M3**：`numbers`(L4) + `body`(L5 整体加密) + `antidbg`(L7)
- [ ] **M4/M5**：**VM（L6）**：`vmgen/isa.rs` + `compiler.rs` + `template.rs`
  + VMC 每构建随机化
- [ ] **M6**：CLI 预设（low/medium/high/vm）+ README 产品文档
- 每个 pass 完成后执行强制工作流（矩阵全绿才算完成）

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
├── luraph-rs/                  # ★ Rust 混淆器（M0 完成，round-trip 全绿）
│   ├── Cargo.toml              #   std-only 零依赖
│   ├── src/
│   │   ├── main.rs             #   CLI（--dialect/-o）
│   │   ├── ast.rs              #   AST 定义
│   │   ├── lexer.rs            #   双方言词法（11 单测）
│   │   ├── parser.rs           #   双方言语法（Pratt/注解剥离/去糖）
│   │   ├── symtab.rs           #   作用域解析
│   │   ├── printer.rs          #   打印器（优先级括号/字节精确字符串）
│   │   └── rng.rs              #   Park-Miller PRNG
│   └── tests/
│       ├── cases/*.lua         #   17 个测试语料
│       └── run_tests.sh        #   测试矩阵（62 项检查）
└── lph/                        # 早期 Lua 参考实现（已被 Rust 取代，仅存档）
    ├── rng.lua
    ├── lexer.lua
    └── parser.lua
```
