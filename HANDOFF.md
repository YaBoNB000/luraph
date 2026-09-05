# AI 交接文档（HANDOFF）

> **给新接手的 AI 读**：这份文档让你（从未参与过本项目对话的 AI）在 10 分钟内
> 完整了解：仓库是什么、用户要什么、现在做到哪了、环境怎么用、规矩是什么、
> 下一步该干什么。读完此文件再动手；细节在 `docs/` 里。
>
> **最后更新：2026-08-29**（修复 v15 输出混入 CJK/特殊字符：词法器
> `high_bytes` 标志 → 转义 ≥0x80 字节的字符串按二进制重打印；64 示例
> 0 非 ASCII。v15 阶段 A=操作数散布已完成，安全项全部收官；官方 204
> + 预设 405 + 多 seed 5×8 全绿）

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
   - 跑**官方矩阵** `luraph-rs/tests/run_tests.sh`（当前 204 项）；改
     `vmgen/` 时**必须加多种子回归**（seed 置换是每构建随机的，固定
     seed 42 测不出置换面回归，见 §8 第 16 条）
   - 改 `vmgen/` 发射面时**还要跑安全指纹**
     `python3 tests/security_fingerprint.py <v15输出> <源语料>`（S1–S5
     攻击脚本判定，见 `docs/plan-resemblance-and-security.md`；安全项
     只许逐阶段转绿，不许回红）
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

## 3. 当前状态（2026-08-28）

| 阶段 | 状态 |
|---|---|
| 环境搭建（Rust / Lua 5.1 / Luau 解释器） | ✅ 完成（工具链在仓库内 `.tools/bin`，见 §4） |
| 混淆技术调研 + 双方言语义实测 | ✅ 完成（`docs/obfuscation-research.md`） |
| Luraph v15 样本分析 | ✅ 完成（`docs/luraph15-analysis.md`） |
| VM 设计草案（ISA/解释器模板/VMC 随机面） | ✅ 完成（research 文档 §2.6 + v15 报告 §8 的采纳决策） |
| M0 地基（lexer/parser/AST/symtab/printer round-trip） | ✅ 完成 |
| M1（L1 名称混淆 + **L1 minify 单行压缩** + L2 字符串加密） | ✅ 完成 |
| **M2（L3 控制流：CFG 扁平化状态机 + 循环嵌套子状态机 + junk）** | ✅ **完成（矩阵 68/68 全绿）** |
| **M3（L4 数值 + L5 整体加密 + L7 反篡改）** | ✅ 完成（矩阵 68/68） |
| **M4（L6 VM：私有字节码 + 生成混淆解释器，`--vm`）** | ✅ **完成 + 续期加固（2026-08-25）：矩阵 204/204 全绿（语料 21→29，新增 8 个 stress_*）+ 多种子回归（5 seeds）0 失败；upvalue 单 cell 别名模型 / 循环变量 per-iteration 共享 cell / 5.1 构造器存储序 / 全变长展开等 9 类语义修复；实现笔记 `docs/vm-l6-implementation.md` §8** |
| **M5（VM 完整随机面）** | ✅ **完成（2026-08-25）**：SoA 平行数组 + 完整 7/14/21-bit + base-94 载体/token + 解码枢纽/状态元组随机 + 帧入场原语解包 + `luac51 -l` 抽查；矩阵 204/204 + 多种子 0 失败 |
| **「看着像+安全性也像」战役** | ✅ **主体完成（2026-09-05）**：三个致命缺点闭环，安全指纹 **0/5 → 5/5**；**P4 防御代码隐藏专项**：防御代码全部运行时构造（打乱码表+序表拼接、数字槽原语、`getfenv(0)[...]` 索引），明文残留清零；**P5 staging 去重复**：`(C,f1..f5)`/`p1..p5` 连块全部随机化（状态寄存器+参数名+诱饵名每构建随机），修两隐患（谓词字面 V 随重命名失效、名称池重复项致状态链死循环）。P0 安全指纹 S1–S5 + 攻击脚本；P1 常量加密+动态内联（S1）；P2 字节码不规则化：节化乱序 + 位置掩码 + 假节 + 流加噪（S2）；P3a handler 体加密 + loadstring 原生复检引导 + CPS 信号化（S3/S5）；P3b OC 运行时生长 + BW 拆槽掩码 + LCG 变形（S4）；P3c 口径定稿 + 诱饵分发点（flatten 实测回退：样本 VM 内核是循环分发非扁平状态机）。**防静态加强·增量⑨（对照破解报告 R001）**：破解 AI 纯静态完整还原 print(12)（本质=可见钥匙的确定性算术可离线重放）；⑨-1 常量块全字节掩码 + ⑨-2 base94 字母表/转义表去物化（APH/TKD 掩码数组+开机重建），断静态脱壳链前两环，攻击脚本现卡在字母表重建、常量恢复 0。余下：恒真谓词/硬编码校验和（⑪）、密钥字面量（⑩）、孪生解密器/键集空洞/诱饵/品牌串（⑫）。双脚本门禁：`v15_fingerprint.py`（轮廓 32 条）+ `security_fingerprint.py`（安全 5 项），改 `vmgen/` 发射面必跑。计划 `docs/plan-resemblance-and-security.md` |
| **M6（产品化）** | ✅ **完成（2026-08-25）**：`--preset low\|medium\|high\|vm\|max`（默认 ≡ high；vm ≡ `--vm`；max = vm，v2 预留）+ 产品 README + `docs/performance.md`；预设矩阵 405/405 |
| **v15 结构同族（路线 A）** | ✅ **结构 100% 还原达成（2026-08-29）**：安全项全部收官后，用户改拍板「先结构 100% 还原」→ 结构战役 **增量 E1–E5 全部完成：32/32 指纹 × 30 语料 × 5 种子（150/150 运行时 + 30/30 指纹）+ 矩阵 204/204 + 预设 405/405 + 多 seed 全绿**。E1 复合赋值折叠+脚手架形态（F11/F12/F23/F24/F26/F28/F30/F31）；E2 宽参数 7 参 handler + Luau if 表达式（F6/F22）；E3 内联 -128 阶梯 + 操作数校验和（F13）；E4 RC 载体长串 + HB 十六进制分块长串 + RT 反查表（F8）；E5 解释器字符串池 MS/TK-CHAR 数值化（F10 233→33）；规模地板（小源码也保量级）+ 诱饵槽碰撞三轮修复 + 长串 5.1 嵌套选级修复。**下一步：回到安全增强（阶段 D 每帧闭包/动态分析阻力）**。详见 `docs/v15-pipeline-rewrite.md` + implementation-plan 更新日志 |

**M5 清单**（对照 `docs/vm-l6-implementation.md` §7，全部勾完）：

- ✅ 随机决策树分派 / Nop 死指令 / slot_perm 操作数槽随机（M4 续期已有）
- ✅ SoA 平行数组（`[ncode][W bytes][S0..S3 varint]`；pc 按指令步进；
  数组名必须是 `W` 不能叫 `OC`——会冲掉 opcode 名表）
- ✅ 7-bit 完整档（7/14/21/28-bit + 128 进制 + 2³² 归一化）
- ✅ 解码枢纽/状态元组位置每构建随机（inline/`hub()` + 6 元组序 +
  `run` 字段序 + helper 声明序）
- ✅ base-94 + 保留前缀 token 转义（v15 pC 同款 10 特殊字符）
- ✅ 帧运行器入场原语解包（15 原语 → `P[1..80]` 随机槽，顶层 + run 双解包）
- ✅ 反编译人工抽查（`luac51 -l` 仅见 L5 容器；用户串不可检索）

**继续开发时**没有未完成的里程碑。新功能按需加，每项仍走 §2.2 强制
工作流（官方矩阵 + `run_presets.sh` + 改 `vmgen/` 时多种子 + md 同步 +
commit/push）。

## 4. 环境（全部已就绪，路径如下）

```bash
# ⚠️ 2026-08-25 起工具链在**仓库内**（沙箱重置会抹掉 /home/user/tools，
# 仓库内副本持久化；run_tests.sh 自动回退到仓库内路径）
/home/user/luraph/.tools/bin/lua51          # Lua 5.1.5
/home/user/luraph/.tools/bin/luac51         # luac 5.1.5
/home/user/luraph/.tools/bin/luau           # Luau 0.735（注意：不支持 -e，用临时文件跑）
/home/user/luraph/.tools/bin/luau-compile   # Luau 语法/字节码编译校验

# Rust（注意：PATH 里可能没有，用全路径或 export PATH=/home/user/luraph/.tools/bin:$PATH）
/home/user/luraph/.tools/bin/rustc          # 1.88.0 stable
/home/user/luraph/.tools/bin/cargo          # 1.88.0
```

验证环境：`/home/user/luraph/.tools/bin/lua51 -v && /home/user/luraph/.tools/bin/luau -v
2>&1 | head -1 && /home/user/luraph/.tools/bin/rustc --version`

### ⚠️ 沙箱网络限制（踩过的坑，别重踩）

- **可达**：api.github.com、codeload.github.com（仓库 tarball）、
  registry.npmjs.org（npm tarball）、pypi.org/files.pythonhosted.org、luau.org（仅 fetch 类工具）
- **不可达**：static.rust-lang.org、sh.rustup.rs、crates.io、static.crates.io、
  index.crates.io、deb.debian.org（apt 不可用）、github release assets CDN、
  lua.org（GET 被断）、国内 Rust 镜像
- **直接后果**：
  - **Rust 项目必须 std-only**（拿不到任何第三方 crate）。这是设计约束，不是缺点
  - 需要新工具时：先想 npm 包 / GitHub codeload tarball / PyPI 三条路
  - Rust 工具链就是靠 npm 上的 `@rustbin/*` 包装的（仓库内 `/home/user/luraph/.tools/lib/`）

### ⚠️ 沙箱重置后的工具链重建（2026-08-25 实战过一遍，全部进仓库）

`/home/user/tools/` 在沙箱重置后会丢（只有 `/home/user/luraph/` 仓库持久）。
**现行工具链已整体建在仓库内** `/home/user/luraph/.tools/`（2026-08-25
重建，重置后若被抹掉再按下面重建；`run_tests.sh` 优先
`/home/user/tools/bin`、自动回退仓库内路径）：

```bash
T=/home/user/luraph/.tools && mkdir -p $T/bin $T/lib
cd /tmp

# 1) Rust 1.88（npm @rustbin 版本锁定包名；每个 tgz 解到独立目录，
#    解同一目录会互相覆盖！）
for p in rustc cargo rust-std; do
  npm pack @rustbin/$p-1.88.0-x86_64-unknown-linux-gnu >/dev/null 2>&1 || true
  mkdir -p x_$p && tar xzf rustbin-$p-*.tgz -C x_$p
  mkdir -p $T/lib/$p && cp -r x_$p/package/. $T/lib/$p/
done
# rust-std 合入 rustc 的 sysroot（cargo 需要）
cp -rn $T/lib/rust-std/rust-std-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/. \
       $T/lib/rustc/rustc/lib/rustlib/x86_64-unknown-linux-gnu/
ln -sf ../lib/rustc/rustc/bin/rustc $T/bin/rustc
ln -sf ../lib/cargo/cargo/bin/cargo $T/bin/cargo

# 2) Lua 5.1.5（lua.org 不可达，用 GitHub 镜像源码编译；
#    无 readline 头文件 → make generic，别用 linux 目标）
curl -sL -o l.tar.gz https://codeload.github.com/zgpxgame/lua-5.1.5/tar.gz/refs/heads/master
tar xzf l.tar.gz && cd lua-5.1.5-master/src && make generic MYLDFLAGS="-ldl -lm" -j4
cp lua $T/bin/lua51 && cp luac $T/bin/luac51

# 3) Luau 0.735（无 cmake 时的 g++ 直编路线：7 个库 + 自写最小 main。
#    自写 main 在仓库内 luraph-rs/tools/luau-cli-mains/（/tmp 会被重置
#    抹掉，勿只存 /tmp！），必须复刻官方 CLI 运行环境，否则语料假通过/
#    假失败：luaL_openlibs + 自定义 loadstring/collectgarbage 全局 +
#    require 文件 rehook（is_require_allowed/reset/jump_to_alias/to_parent/
#    to_child/is_module_present/get_chunkname/get_loadname/get_cache_key/
#    get_config_status/get_alias/load 全指针）+ luaL_sandbox。
#    注意：Require 库依赖 Config 库 → 源码列表必须含 Require/src/*.cpp
#    + Config/src/*.cpp，include 必须含 Require/include + Config/include，
#    否则 undefined reference to luaopen_require 等）
curl -sL -o luau.tar.gz https://codeload.github.com/luau-lang/luau/tar.gz/refs/tags/0.735
tar xzf luau.tar.gz && cd luau-0.735
cp /home/user/luraph/luraph-rs/tools/luau-cli-mains/main_luau*.cpp .
SRC=$(ls VM/src/*.cpp Common/src/*.cpp Ast/src/*.cpp Bytecode/src/*.cpp \
       Compiler/src/*.cpp Require/src/*.cpp Config/src/*.cpp)
g++ -O2 -std=c++17 -DLUA_USE_LONGJMP=1 '-DLUA_API=extern "C"' \
  -I VM/include -I Common/include -I Ast/include -I Bytecode/include \
  -I Compiler/include -I Require/include -I Config/include \
  main_luau.cpp $SRC -o $T/bin/luau -pthread
# luau-compile 同法（main 只做 compile + luau_load 校验，输出字节码到
# stdout；不用 require，可只编前 6 个库）
```

**两个环境级语义（语料与用户程序必须遵守，2026-08-25 实测）**：
- 官方 luau CLI **沙箱化全局表**（`luaL_sandbox`，0.600+ 均如此）：顶层
  `newkey = v` 报 `attempt to modify a readonly table`。共享语料不得含
  顶层新建全局赋值。
- **for-in 方言差异**：Luau 支持 `for k,v in t`（隐式 next）；5.1 迭代器
  必须可调用（裸 table 运行时 call-a-table 错误 = 正确行为）。


## 5. 仓库文件地图（2026-08-25 实际树）

```
luraph/
├── HANDOFF.md                  # ← 本文件（新 AI 先读这个）
├── PROGRESS.md                 # 项目进度（每个里程碑一节，M4 续期在 §M4 续期）
├── README.md                   # 产品文档（预设 / 用法 / 性能摘要）
├── .tools/                     # ★ 工具链（gitignored，708M；重建步骤见 §4）
│   └── bin/{rustc,cargo,lua51,luac51,luau,luau-compile}
├── docs/
│   ├── obfuscation-research.md # ★ 分层混淆体系 L1–L7、VM 设计草案、双方言
│   │                           #   语义实测表、优先级表、坑清单、语料清单、验收
│   ├── implementation-plan.md  # ★ 实施清单（§1 状态列）+ 进度表（§2）+
│   │                           #   里程碑（§3）+ 更新日志（§4，倒序）
│   ├── luraph15-analysis.md    # ★ Luraph v15.0 样本逆向分析（架构/脱壳/采纳决策 §8）
│   ├── v15-structural-parity-plan.md # ★ 与样本「结构 100% 同族」的分阶段计划（2026-08-25）
│   ├── performance.md          #   M6 预设性能快照
│   └── vm-l6-implementation.md # ★★ VM 实现笔记：架构/多值协议/upvalue 单 cell
│                               #   模型（§8.1）/9 类 bug（§8.2）/环境语义（§8.3）/
│                               #   M5 清单（§7）/调试工具箱（§6）—— 动 VM 前必读
├── samples/                    # v15 动态分析工件（仅参考）
│   ├── luraph15.txt            #   用户提供的 Luraph v15.0 混淆样本（171KB minified）
│   ├── luraph15.lua / _trace.lua  #   带 Roblox 环境 stub 的可跑版本 + 插桩版
│   └── make_trace.py / run_*.lua / t1*.lua / mod1,2 / polyfill  # 脱壳探针脚本
├── lph/                        # 早期过渡参考代码（Lua 写的，仅参考，不复用）
│   ├── rng.lua                 #   Park-Miller PRNG（Rust 版照抄算法）
│   ├── lexer.lua               #   5.1+Luau 词法（转义/长串/反引号插值细节）
│   └── parser.lua              #   递归下降 + Pratt（语法边界按 lparser.c 核对过）
└── luraph-rs/                  # ★ Rust 混淆器本体（std-only，edition 2021）
    ├── src/
    │   ├── main.rs             #   CLI（--preset/--dialect/--seed/--vm/--no-*）+ 管线装配
    │   ├── lexer.rs / parser.rs / ast.rs / symtab.rs / printer.rs   # M0 地基
    │   │                       #   （printer 的 Ctx::Suffix = 后缀位置括号规则）
    │   ├── rng.rs / rng_check.rs                                # 种子 PRNG
    │   ├── minify.rs / mangle.rs / strings.rs                   # L1 / L1 / L2
    │   ├── flatten.rs / junk.rs                                 # L3（flatten 内含
    │   │                       #   循环降级 make_loop —— ForGen 单迭代器语义在这）
    │   ├── numbers.rs / body.rs / antidbg.rs                    # L4 / L5 / L7
    │   ├── guard.rs                                             # 反调试环境完整性 guard（2026-08-29，用户设计）：标准管线 = mangle 前奏；v15 = CHAR 编码注入 FC 入口机头部（指纹零破坏）；--no-guard 两路同关
    │   ├── desugar.rs        # ⚠️ 孤儿文件（main.rs 未声明 mod，死代码，勿改勿依赖）
    │   └── vmgen/            # ★ L6 VM
    │       ├── isa.rs        #   42 条 Op（含 P1 MkStr）+ OpMap + SoA + 7-bit
    │       │                 #   + Carrier + 常量加密（ConstLcg/dyadic/varint64）
    │       ├── compiler.rs   #   AST→字节码（单 cell + 方言分支 + 指令下标跳转）
    │       ├── template.rs   #   解释器装配（SoA/decarrier/hub/原语解包/决策树；
    │       │                 #   指令体向 handlers/ 逐条取码 + 每构建选形/打乱）
    │       ├── strpool.rs    #   解释器字符串池（v15 MS 表 / legacy 内联字面量）
    │       ├── handlers/     #   ★ 建议1：41 条指令各一文件，每文件返回固定
    │       │                 #   解释器代码；每指令 2~3 形态每构建随机选一
    │       └── mod.rs
    ├── tests/
    │   ├── run_tests.sh        # ★ 官方矩阵（非 VM + VM 两阶段；.tools 路径回退）
    │   ├── run_presets.sh      # ★ 五档预设 × 全语料（M6，405 项）
    │   ├── v15_fingerprint.py  # ★ v15 结构指纹 32 条（P0；样本 32/32，改
    │   │                       #   vmgen/ 发射外壳前后对打用；解析口径坑见文件头）
    │   ├── bench_presets.sh    #   性能快照 → docs/performance.md
    │   ├── multiseed.sh        # ★ 多种子回归（VM 改动必跑；seeds 可传参，
    │   │                       #   默认 1 7 4242 31337 999999）
    │   ├── gen_examples.sh     #   示例再生成（seed 42，VM 子集 3 个）
    │   └── cases/*.lua         # ★ 29 个语料（20 共享 + 9 luau_*；8 个 stress_*）
    └── tools/
        └── luau-cli-mains/     # ★ 重建 .tools 的 luau/luau-compile 自写 main
                                #   （/tmp 会被重置抹掉，唯一权威副本在这里）
    └── examples/               # 混淆示例（<case>.5.1.lua / .luau.lua / .vm.5.1.lua）
```

**改代码的入口**：新混淆 pass → `src/` 加模块并在 `main.rs` 挂管线；VM 相关
→ `src/vmgen/`（改模板前必读 `docs/vm-l6-implementation.md` §8，upvalue
描述符三形态的改动极易回归）；语料 → `tests/cases/`（共享语料的双方言红线
见 §8 第 15 条）。

## 6. 核心知识速览（细节去读 docs/）

### 6.1 双方言关键差异（全部实测过，2026-08-23 初测 + 2026-08-25 补 for-in/沙箱两行）

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
| for-in 裸 table | **非法**（迭代器必须可调用，`for k,v in t` 运行时
  call-a-table 报错） | **合法**（隐式 `next, t`，语言级扩展；parser 在 Luau
  档归一化，5.1 档原样透传） |
| 顶层新建全局 | 合法 | **报错**（CLI 的 `luaL_sandbox` 全局只读） |

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

### 6.4 VM 设计要点（L6，详见 `docs/vm-l6-implementation.md`）

> 🚧 2026-09-05 起进入「看着像 + 安全性也像」战役：三个致命缺点
> （解释器明文暴露 / 字节码规整 / 常量明文）四阶段整改，总计划见
> `docs/plan-resemblance-and-security.md`（P0 指纹改版 → P1 常量 →
> P2 字节码 → P3 解释器分层）。

- 寄存器 VM（41 条指令，`vmgen/isa.rs`），SoA 平行数组 + 7/14/21-bit
  varint；跳转 = 1 基指令下标
- 输出 = 运行时加载器（解密）+ 自定义解释器（Lua 源码，**再过全套混淆
  管线**）+ base-94/token 载体包裹的加密字节码 + 入口
- **每构建随机化（VMC，M5 全开）**：opcode 置换表 / 随机决策树 /
  Nop / 完整 7-bit / slot_perm / SoA / 枢纽风格与元组序 / 原语槽号 /
  载体字母表与 token / **指令多形态**（建议1：`vmgen/handlers/` 41 文件，
  每指令 2~3 语义等价形态每构建各选一 + 分派叶序/定义序/调用链序每生成打乱）
- 密钥流：状态机化 LCG PRNG（mod 2²⁸/2³¹-1，常数每构建随机）
- 元表/协程/pcall：透传宿主 VM（不模拟，正确性优先）
- **upvalue 单 cell 模型（2026-08-25 换代，动 upvalue 前必读 §8.1）**：
  5.1 语义 = 每个 local 全程序只有一个 cell，所有闭包引用同一 cell。
  描述符三形态（`upsrc` 表里的 u16）：
  - `slot`（< 32768）：普通 = `{ v = V, i = slot }`（创建帧活槽位）
  - `0x8000 | slot`：循环体局部/循环变量被闭包捕获 → 每迭代在 `V[slot]`
    建 cell 表 `{1=value}`，makefn 绑 `{ v = V[slot], i = 1 }`
    （同迭代闭包 + 循环体共享；fresh per iteration）
  - `0xC000 | upidx`：创建帧自己 materialize 了该符号 → 闭包**直接别名
    父帧的 cell 对象**（`c[i] = upsf[upidx]`；makefn 需显式传入父帧
    cell 数组——词法作用域看不到 run 的局部）。materialize 是纯作用域
    别名，**不发射任何指令**（旧模型是 GetUp 值副本 + 写回转发，已废）
- 多方言分支点：构造器存储序（5.1 位置字段最后落表 / Luau 源码序，
  `compiler.rs` 按 `lua51` 标志）、for-in 隐式 next（parser Luau 档
  归一化）、`_G` 只读（VM 用 `getfenv(0)`）、`#` 元方法运行期探测
- 我方差异化（v15 没有的）：**纯 Lua 数据层保 5.1+Luau 双目标** + 校验和 + 时间陷阱

### 6.5 Luraph v15 样本结论（一句话版）

三层分发（142 handler）+ 7-bit 分块寄存器 + LCG 状态机密钥流 +
Roblox buffer 数据层 + base-N token 转义 + 每帧 Lua 闭包 + 超级指令；
弱点：无时间陷阱/显式校验和，动态 hook 仍可行。→ 全部细节在
`docs/luraph15-analysis.md`。

## 7. 开工顺序（M0–M6 已完成）

**接手第一步**（环境自检，1 分钟）：

```bash
cd /home/user/luraph
# 1) 工具链在不在（沙箱重置若抹掉 .tools/，按 §4 重建步骤恢复）
ls .tools/bin/   # 应有 rustc/cargo/lua51/luac51/luau/luau-compile
# 2) 构建 + 矩阵（预期 PASS 204 / FAIL 0 / ALL GREEN）
#    ⚠️ 必须先 export PATH（cargo 靠 PATH 找 rustc，全路径 cargo 也不行）
export PATH=/home/user/luraph/.tools/bin:$PATH
cd luraph-rs && CARGO_NET_OFFLINE=true cargo build --release
bash tests/run_tests.sh
# 3) 多种子回归（tests/multiseed.sh，仓库内；改 vmgen/ 必跑）
bash tests/multiseed.sh
```

**M0–M6 已完成。** v15 结构同族**已拍板路线 A**（Roblox/Luau 克隆档，
2026-08-25）：P0 脚手架 ✅（`tests/v15_fingerprint.py` 32 条：样本全绿 /
现 `vm` 产物全红；`--preset v15` = Luau 门控 + stub），**下一步 = P1 外壳
换代**——全部路线/阶段/验收在 `docs/v15-structural-parity-plan.md`，
改发射外壳前必读其 §1.1（五处架构修正）。

**通用规则**：改 VM 三件套（isa/compiler/template）任何一处 → 先读
`docs/vm-l6-implementation.md`（尤其 §8 的 9 类坑），改完 = 矩阵 204 +
多种子回归 + 4 个 md 同步 + commit/push，缺一不可。

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
8. 多值语义：`a,b=f()` / `local a,b=f()` / `return a,b` / `{f()}` 尾部
   展开；**变长也要全展开**：`return ...` / `a,b = ...` / `a,b = f(...)`
   （M4 续期修过：原实现只取第一个 vararg）；多余值「求值即弃」（副作用
   保留，如 `local a = 1, print("x"), f()` 的 print 必须执行）
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
12. 工作分支以当时环境说明为准（本会话 `arena/01a038ae-luraph`），别动 main
13. Rust 构建：`export PATH=/home/user/luraph/.tools/bin:$PATH && cd luraph-rs && CARGO_NET_OFFLINE=true cargo build --release`（PATH 必须先导出——cargo 靠 PATH 定位 rustc，光用全路径 cargo 会报 `could not execute process rustc -vV`）
14. **luau CLI 沙箱**（2026-08-25 实测）：官方 luau CLI（0.600+）全局表
    只读（`luaL_sandbox`）→ 顶层**新建**全局赋值 = `attempt to modify a
    readonly table`。**共享语料（非 luau_* 前缀）不得含顶层新建全局赋值**
    （会挂 cross 检查：5.1 能建、Luau 不能建，原始程序双方言输出就不同）
15. **双方言红线（共享语料必须两侧行为一致，否则 cross 必挂）**：
    ① 重复键表构造器（`{10, [1]=11}` 类）——5.1 位置字段必胜（SETLIST
    最后落表）、Luau 源码序最后写入胜；② 顶层新建全局赋值（见第 14 条）；
    ③ 未捕获错误的裸报错行（两边行号/格式不同）——用 pcall 包住只打
    type/结果；④ 大数/特殊浮点的 tostring 格式差异。luau_* 前缀语料无
    cross 检查，可放 Luau 专属语义
16. **VM 改动后的回归面**：矩阵（`tests/run_tests.sh`，204 项）只固定
    seed 42——opcode 置换/槽位排列是每构建随机的，**必须加多种子回归**
    （权威脚本 = `tests/multiseed.sh`，seeds 可传参，默认
    1 7 4242 31337 999999）。逻辑参考（新会话没有脚本时按此内联）：
    ```bash
    for seed in 1 7 4242; do for case in luraph-rs/tests/cases/*.lua; do
      base=$(basename $case .lua); d=5.1; [ "${base#luau_}" != "$base" ] && d=luau
      I=$L/lua51; [ $d = luau ] && I=$L/luau   # L=.tools/bin
      o1=$(timeout 60 $I $case 2>&1); c1=$?
      $TOOL --vm --dialect $d --seed $seed $case /tmp/o.lua
      o2=$(timeout 60 $I /tmp/o.lua 2>&1); c2=$?
      [ "$c1" != "$c2" ] || [ "$o1" != "$o2" ] && echo "FAIL $base $d $seed"
    done; done
    ```
17. **调试 VM 的三板斧**（`docs/vm-l6-implementation.md` §6 有完整工具箱）：
    ① `LURAPH_VM_RAW=1` 出未混淆解释器（行号可读、可直接插 print）；
    ② 在生成文件的 `makefn`/分派分支里插 `io.write` 追踪（oc/寄存器值）
    ——**插桩脚本自己出过错：跨行拼接要按整行替换，别用 index 硬拼**；
    ③ 二分 pass（`--no-flatten/--no-strings/...` 逐个关）定位管线交互
    bug。字节码反汇编：`cargo test dbg -- --nocapture`（读
    `/tmp/mv1.lua`，要改路径的话在 `vmgen/compiler.rs` 末尾测试里）
18. **shell 变量坑**（本轮实际踩过）：`/tmp/t_$s_$d.lua` 里 `$s_` 会被
    解析成名为 `s_` 的变量（空）——拼接时加花括号 `${s}_${d}`

## 9. 验收标准（「商业级」的落地定义）

1. 全部测试语料 × 双解释器 × 全预设：stdout + 退出码 100% 一致
   （当前矩阵 204 项 = 非 VM 102 + VM 102，含 5.1→luau 交叉）
2. 输出经 `lua51 loadstring` / `luau-compile` 语法校验 0 错误
3. 同 `--seed` 两次构建逐字节一致；不同 seed 字节码编码完全不同；
   **多种子回归**（≥5 个 seed × 全语料 × 双方言 × 双阶段）0 失败
4. VM 预设输出：标准反编译器/格式化器无法恢复源码结构（人工抽查）
5. 反篡改生效：篡改任一密文段 → 触发陷阱
