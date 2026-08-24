# AI 交接文档（HANDOFF）

> **给新接手的 AI 读**：这份文档让你（从未参与过本项目对话的 AI）在 10 分钟内
> 完整了解：仓库是什么、用户要什么、现在做到哪了、环境怎么用、规矩是什么、
> 下一步该干什么。读完此文件再动手；细节在 `docs/` 里。

---

## 1. 项目是什么

**商业级 Lua 混淆器**（对标 Luraph，Roblox 生态最强的商业混淆器），
仓库名即产品名：**luraph**。

核心卖点（用户明确要求）：
- **自带自定义 VM + 自己的解释器**（字节码虚拟化：用户源码 → 私有字节码 →
  嵌入输出脚本的「Lua 写的解释器」执行，标准反编译器完全失效）
- **双目标方言：Lua 5.1 和 Luau**（输出脚本在两种解释器上都能运行）
- **混淆器本体用 Rust 编写**（std-only，零第三方依赖）
- 商业级 = 多层纵深防御 + 每构建唯一 VM + 反篡改

## 2. 用户的硬性要求（必须遵守）

1. **在用户明确说「开始」之前，不要编写混淆器代码**（写文档/环境/调研可以）
2. **每次更改完混淆模块后**（强制工作流，用户 2026-08-23 再次强调）：
   - 产出一个混淆脚本（用测试语料跑当前已实现的全部 pass）
   - 用 `lua51` 和 `luau` **两个解释器**验证：语法（`loadstring`/`luau-compile`）
     + 实际运行（stdout 与退出码和原始脚本一致）
   - 语料必须覆盖**所有常用语法**（清单见 `docs/obfuscation-research.md` 第 6 节）
   - 任一项不通过 = 该模块改动未完成，不许宣称完成
   - **更新所有相关 md**（implementation-plan 四处 + PROGRESS + HANDOFF +
     examples/README，见 §8 第 11 条）
   - **commit + push 到 GitHub**（分叉时先 merge 远端，勿 force push）
3. 混淆器语言 = **Rust**（不要用 Lua/其他语言写混淆器本体；`lph/` 里的 Lua
   代码只是早期参考实现，**不复用**）
4. 学习/调研的结论要**写入笔记**（`docs/`），用户会随时检查笔记
5. 用户提供的 Luraph 样本已分析完毕（`docs/luraph15-analysis.md`），
   VM 设计已吸收其 v15 的关键技术（7-bit 分块/三层分发/LCG 密钥流等）

## 3. 当前状态（2026-08-24）

| 阶段 | 状态 |
|---|---|
| 环境搭建（Rust / Lua 5.1 / Luau 解释器） | ✅ 完成 |
| 混淆技术调研 + 双方言语义实测 | ✅ 完成（`docs/obfuscation-research.md`） |
| Luraph v15 样本分析 | ✅ 完成（`docs/luraph15-analysis.md`） |
| VM 设计草案（ISA/解释器模板/VMC 随机面） | ✅ 完成（research 文档 §2.6 + v15 报告 §8 的采纳决策） |
| M0 地基（lexer/parser/AST/symtab/printer round-trip） | ✅ 完成 |
| M1（L1 名称混淆 + **L1 minify 单行压缩** + L2 字符串加密） | ✅ 完成 |
| **M2（L3 控制流：CFG 扁平化状态机 + 循环嵌套子状态机 + junk）** | ✅ **完成（矩阵 68/68 全绿）** |
| **M3（L4 数值 + L5 整体加密 + L7 反篡改）** | ✅ 完成（矩阵 68/68） |

**继续开发时**按 `docs/implementation-plan.md` §3 的里程碑顺序（M3 → M4/M5
VM → M6 产品化）。每个 pass 完成后执行 §2.2 强制工作流（矩阵全绿 +
examples 重新生成）。

## 4. 环境（全部已就绪，路径如下）

```bash
# 解释器（源码编译，已验证可用）
/home/user/tools/bin/lua51          # Lua 5.1.5
/home/user/tools/bin/luac51         # luac 5.1.5
/home/user/tools/bin/luau           # Luau 0.735（注意：不支持 -e，用临时文件跑）
/home/user/tools/bin/luau-compile   # Luau 语法/字节码编译校验
/home/user/tools/bin/luau-analyze   # Luau 静态分析

# Rust（注意：PATH 里可能没有，用全路径或 export PATH=/home/user/tools/bin:$PATH）
/home/user/tools/bin/rustc          # 1.88.0 stable
/home/user/tools/bin/cargo          # 1.88.0
```

验证环境：`/home/user/tools/bin/lua51 -v && /home/user/tools/bin/luau -v 2>&1 | head -1
&& /home/user/tools/bin/rustc --version`

### ⚠️ 沙箱网络限制（踩过的坑，别重踩）

- **可达**：api.github.com、codeload.github.com（仓库 tarball）、
  registry.npmjs.org（npm tarball）、pypi.org/files.pythonhosted.org、luau.org（仅 fetch 类工具）
- **不可达**：static.rust-lang.org、sh.rustup.rs、crates.io、static.crates.io、
  index.crates.io、deb.debian.org（apt 不可用）、github release assets CDN、
  lua.org（GET 被断）、国内 Rust 镜像
- **直接后果**：
  - **Rust 项目必须 std-only**（拿不到任何第三方 crate）。这是设计约束，不是缺点
  - 需要新工具时：先想 npm 包 / GitHub codeload tarball / PyPI 三条路
  - Rust 工具链就是靠 npm 上的 `@rustbin/*` 包装的（`/home/user/tools/rust-1.88/`）

## 5. 仓库文件地图

```
luraph/
├── HANDOFF.md                  # ← 本文件（新 AI 先读这个）
├── PROGRESS.md                 # 项目进度（已完成/未开始/目录结构，每次重要节点更新）
├── README.md                   # 仅标题（待混淆器完成后重写为产品文档）
├── docs/
│   ├── obfuscation-research.md # ★ 核心笔记：分层混淆体系 L1–L7、VM 设计草案、
│   │                           #   双方言语义实测表、优先级表、坑清单、Rust 模块规划、
│   │                           #   测试语料清单（第 6 节）、验收标准
│   └── luraph15-analysis.md    # ★ Luraph v15.0 样本逆向分析（架构/别名表/位置索引/
│                               #   脱壳评估/对本项目的设计采纳决策 §8）
├── samples/
│   └── luraph15.txt            # 用户提供的 Luraph v15.0 混淆样本（171KB，minified）
└── lph/                        # 早期过渡参考代码（Lua 写的，仅参考，不复用）
    ├── rng.lua                 #   Park-Miller PRNG（Rust 版照抄算法即可）
    ├── lexer.lua               #   5.1+Luau 词法（转义/长串/反引号插值细节都在这）
    └── parser.lua              #   5.1+Luau 递归下降 + Pratt 优先级（未测试过，
                                #   但语法边界是按 5.1 lparser.c 和 Luau Parser.cpp 核对的）
```

**写新代码的位置**：Rust 项目建议建在 `luraph-rs/`（模块结构见
`docs/obfuscation-research.md` §5）。测试语料放 `luraph-rs/tests/cases/*.lua`，
测试驱动放 `luraph-rs/tests/run_tests.lua` 或 Rust 集成测试。

## 6. 核心知识速览（细节去读 docs/）

### 6.1 双方言关键差异（全部实测过，2026-08-23）

| 特性 | Lua 5.1 | Luau 0.735 |
|---|---|---|
| 全局 `load` | 有 | **没有**（只有 `loadstring` → 运行时加载统一用 loadstring） |
| `%` 取模 | floor（-1%3=2） | floor（同，解密算术可共用） |
| `//` | 无 | `math.floor(a/b)`（-7//2=-4） |
| goto/标签 | 无 | 0.735 也没有（输入遇到就报错） |
| 位运算（值级） | 无 | 无（仅类型系统用 & \|） |
| continue | 无 | 有（上下文关键字，仅语句位置） |
| 复合赋值 `+=` | 无 | 有（去糖为 `a = a + b`） |
| 字符串插值 | 无 | **反引号** `` `a {expr} b` ``（字面花括号用 `\{`，`{{` 报错；
  去糖为 `string.format`，模板里 `%` 要转 `%%`） |
| 类型注解/`type X=` | 无 | 有（解析期剥离） |
| int/float 区分 | 无 | 有（AST 数字节点要带 isfloat 标志，
  输出时 float 字面量必须保留小数点） |

### 6.2 运算符优先级（与 5.1 lparser.c / Luau Parser.cpp 核对过）

```
or(1) and(2) 比较(3) ..(5,右) +- (6) * / % //(7=乘法级) 一元(8) ^(10,右)
```
Pratt 表驱动：`subexpr(limit)`，左优先 > limit 才结合，右操作数用右优先递归。
易错点必须进测试语料：`-2^2=-(2^2)`、`2^-3`、`not x and y`、`1..2..3`。

### 6.3 分层体系（本项目的混淆架构）

L1 名称混淆 → L2 字符串加密（5.1 无位运算 → add8 算术密码或 256×256 查找表 XOR）
→ L3 控制流（CFG 扁平化状态机，无 goto 的 5.1 安全形态；flatten 原生处理
  for/repeat/while/break/continue，无需去糖——desugar 已取消）
→ L4 数值（整数拆分；浮点只做精确恒等变换）→ L5 整体加密（loadstring）
→ **L6 自定义 VM（核心护城河）** → L7 反篡改（校验和/时间陷阱）。
**商业级预设 = L1+L2+L3+L5+L6+L7 全开。**

### 6.4 VM 设计要点（L6，详见 research §2.6 + v15 报告）

- 寄存器 VM（~40 条指令），SoA 存储（opcode/操作数分存不同数组）
- 输出 = 运行时加载器（解密）+ 自定义解释器（Lua 源码，经 L1/L2 混淆）
  + 加密字节码 + 入口
- **每构建随机化（VMC）**：opcode 置换表、随机派发树（2~4 层，v15 同款）、
  7-bit 分块操作数编码（可选档）、死指令、随机命名
- 密钥流：状态机化 LCG PRNG（mod 2²⁸/2³¹-1，常数每构建随机）
- 元表/协程/pcall：透传宿主 VM（不模拟，正确性优先）
- 我方差异化（v15 没有的）：**纯 Lua 数据层保 5.1+Luau 双目标** + 校验和 + 时间陷阱

### 6.5 Luraph v15 样本结论（一句话版）

三层分发（142 handler）+ 7-bit 分块寄存器 + LCG 状态机密钥流 +
Roblox buffer 数据层 + base-N token 转义 + 每帧 Lua 闭包 + 超级指令；
弱点：无时间陷阱/显式校验和，动态 hook 仍可行。→ 全部细节在
`docs/luraph15-analysis.md`。

## 7. 开工顺序（等用户说「开始」后）

1. `cargo new luraph-rs`（零依赖；Rust edition 2021；
   注意 cargo 要带 `CARGO_NET_OFFLINE=true`，且确认 sysroot 的 std 在
   `/home/user/tools/rust-1.88/rustc/lib/rustlib/x86_64-unknown-linux-gnu/`）
2. `rng.rs`（照 `lph/rng.lua` 的 Park-Miller）→ `lexer.rs`（照 `lph/lexer.lua`，
   注意反引号插值/长字符串/转义全部细节）
3. `parser.rs`（Pratt 表见 §6.2；类型注解剥离；复合赋值/`//`/插值去糖）
   → `ast.rs` → `symtab.rs`（作用域/sym/全局名收集）→ `printer.rs`
4. **里程碑 A**：round-trip（输入 → AST → 输出，语义等价）+ 全部测试语料
   过双解释器 → 给用户看
5. 逐个 pass（**实际流水线顺序**，M0–M2 已完成）：
   `junk` → `mangle`(L1) → `flatten`(L3) → `strings`(L2) →（M3 起）
   `numbers`(L4) → `body`(L5) → `antidbg`(L7)
   —— **每加一个 pass 都执行 §2.2 强制工作流**（矩阵全绿 + examples 重新生成）
6. **VM（L6）**：`vmgen/isa.rs`（指令集+编码+随机置换）→
   `vmgen/compiler.rs`（AST→字节码）→ `vmgen/template.rs`（解释器模板生成）
   —— 先做最小可用 ISA（先支持测试语料的子集），扩指令时同步扩语料
7. CLI 预设（low/medium/high/vm）+ README 产品文档

## 8. 规矩与坑（前人踩过的）

1. **不要**在用户说「开始」前写混淆器代码（文档/环境/调研除外）
2. **不要**用 goto/位运算/`//` 写任何 5.1 目标的**输出代码**；
   工具自身源码（Rust）无此限制
3. `lph/*.lua` 如果要在 Lua 里跑，必须 5.1 兼容（无 goto）——
   它们目前只是参考，别改出 5.2+ 语法
4. Luau CLI 不支持 `-e`，验证时写临时 `.lua` 文件再 `luau 文件`
5. 浮点输出用 `%.17g`，Luau 下 float 字面量补 `.0`；整数 |v|<1e15 用 `%d`
6. `return f(...)` 尾调用语义必须原样保留（扁平化不能拆）
7. `local function f() return f() end`：f 在自身体内**不可见**（测试必覆盖）
8. 多值语义：`a,b=f()` / `local a,b=f()` / `return a,b` / `{f()}` 尾部展开
9. 字符串 `\0` 合法，输出用转义字面量；长字符串归一化为带转义短串
10. 所有随机性走种子 PRNG（`--seed` 可复现；同 seed 输出逐字节一致，
    不同 seed 编码完全不同——这是验收标准之一）
11. **每完成一个混淆 pass（用户已强调，必做，缺一不可）**：
    ① 矩阵全绿 + examples 重新生成（§2.2 强制工作流）；
    ② **同步更新所有相关 md**：`docs/implementation-plan.md`（§1 状态列 +
    §2 进度表 + §3 里程碑 + 更新日志，四处一起改）+ `PROGRESS.md`
    （进度/目录结构）+ 本文件（状态表/文件地图，如有变化）+
    `luraph-rs/examples/README.md`（已实现 pass 列表）；
    ③ **`git commit` + `git push origin <工作分支>` 上传 GitHub**
    （历史分叉时先 merge 远端再 push，勿 force push）
    —— 只改代码不更新 md / 不推 GitHub = 该 pass 未完成
12. 工作分支是 `arena/01a02d14-luraph`（本会话）；新会话的分支以当时的
    环境说明为准，别动 main
13. Rust 构建：`cd luraph-rs && CARGO_NET_OFFLINE=true /home/user/tools/bin/cargo build`

## 9. 验收标准（「商业级」的落地定义）

1. 全部测试语料 × 双解释器 × 全预设：stdout + 退出码 100% 一致
2. 输出经 `lua51 loadstring` / `luau-compile` 语法校验 0 错误
3. 同 `--seed` 两次构建逐字节一致；不同 seed 字节码编码完全不同
4. VM 预设输出：标准反编译器/格式化器无法恢复源码结构（人工抽查）
5. 反篡改生效：篡改任一密文段 → 触发陷阱
