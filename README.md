# luraph

商业级 Lua 混淆器（对标 [Luraph](https://lura.ph/)）。把用户脚本编译成**私有字节码**，
由随文件分发的「Lua 写的混淆解释器」执行——标准反编译器拿不到原生 5.1/Luau
字节码，也还原不出源码结构。每个 `--seed` 的 VM（指令置换、派发树、操作数
编码、命名、载体字母表）都不同。

- **双目标**：Lua 5.1 与 Luau，同一条管线，官方矩阵双方言交叉验证
- **本体**：Rust，std-only，零第三方 crate
- **纵深**：L1 名称/压缩 → L2 字符串加密 → L3 控制流扁平化 → L4 数值 →
  L5 整段加密容器 → **L6 私有 VM** → L7 反篡改（金丝雀 / 校验和 /
  行号重映射 / 时间陷阱）

## 构建

需要 `rustc`/`cargo` ≥ 1.88。本仓库沙箱把工具链放在 `.tools/bin`（`gitignored`）：

```bash
cd luraph-rs
export PATH=/home/user/luraph/.tools/bin:$PATH   # 使用仓库内置工具链时
CARGO_NET_OFFLINE=true cargo build --release
```

沙箱外：系统 Rust 即可，不必离线。重建 `.tools` 的步骤见 `HANDOFF.md` §4。

## 用法

```bash
# 默认 = --preset high（L1–L5 + L7，无 VM）
./target/release/luraph-rs --dialect 5.1  --seed 42 in.lua out.lua
./target/release/luraph-rs --dialect luau --seed 42 in.lua out.lua

# 命名预设（后面的 --no-* / --vm 会覆盖）
./target/release/luraph-rs --preset low    --seed 42 in.lua out.lua
./target/release/luraph-rs --preset medium --seed 42 in.lua out.lua
./target/release/luraph-rs --preset high   --seed 42 in.lua out.lua
./target/release/luraph-rs --preset vm     --seed 42 in.lua out.vm.lua
./target/release/luraph-rs --preset max    --seed 42 in.lua out.vm.lua

# v15 结构同族档（Luau/Roblox-only，克隆 Luraph v15 形态）——推荐
# 产物零参数、直接可运行（丢进 Luau CLI / Roblox 执行器即可）
./target/release/luraph-rs --preset v15 --dialect luau --seed 42 in.lua out.v15.luau.lua

# [已弃用] 增量⑱ 输入绑定/激活门：需变长参递送激活值、产物无法直接运行，
# 与「混淆后直接可运行」的产品需求冲突，仅加载器授权场景保留
# ./target/release/luraph-rs --preset v15 --dialect luau --bind-key "KEY" in.lua out.lua

# 等价写法
./target/release/luraph-rs --vm --dialect 5.1 --seed 42 in.lua out.vm.lua
```

| 预设 | 打开的层 | 典型体积 | 适用 |
|---|---|---|---|
| `low` | L1 + L2 | ~源码量级 | 名称/字符串不可读即可 |
| `medium` | low + L3 | 数 KB–数十 KB | 还要打散控制流 |
| `high`（默认） | medium + L4 + L5 + L7 | 数十 KB | 商业级非 VM：整段密文 + 反篡改 |
| `vm` | high + L6 | ~130–160 KB | 反编译器失效；Lua-on-Lua |
| `max` | 当前 = `vm` | 同 `vm` | 最强在售档；v2（CPS 帧/超级指令）预留 |
| `v15` | Luraph-v15 结构同族（Luau/Roblox） | ~140 KB | **推荐**：纯结构防护、零参数直接可运行 |

### 产品形态：混淆后零参数直接可运行（`--preset v15`）

**核心承诺**：`--preset v15` 产物**不需要任何参数**，丢进 Luau CLI 或
Roblox 执行器即可直接运行，输出与源码逐字节一致。防护全部来自**结构层**
（⑲⑳）：

- **加密解释器**：43 个指令 handler + 字节码解析器全部加密成碎片，运行时
  经 `loadstring` 装配，可见代码里没有指令语义；
- **去中心化尾链 + 蹦床**（⑲）：没有中心 `while..dispatch` 分派循环，
  每条指令的分派散在各加密 handler 尾部的尾链附言里，全局 hook 咽喉点消失；
- **多层编码 + 每构建随机**：载体、转义、钥匙表、指令编号、槽位、命名
  每个 `--seed` 全量重随机。

此形态与真实 Luraph v15 同属「纯结构防护」路线（真实 v15 本就无运行时
防御，见 `docs/redteam-luraph15-recon.md`），但本产品把解释器/解析器/分派
全部入密，结构强度更高。

### [已弃用] 增量⑱：输入绑定 / 激活门（`--bind-key`）

曾把引导链第一层门控到「变长参递送的激活值」上（错/缺钥匙即引导层崩溃）。
**因要求运行时传参、产物无法直接运行，与产品核心需求冲突，已弃用**——
代码保留（`--bind-key` 仍可用，仅供加载器授权这一特定场景），但**默认即
直接可运行形态，不要再依赖激活门做防护**。详见
`docs/plan-resemblance-and-security.md`。

### 增量㉑：环境绑定（`--bind-env roblox`，仅 v15）

真实 Luraph v15 的最外层防御。把目标运行时 API（`Vector3.new`/
`Vector2.new`/`task.defer`）嵌进解释器原语槽——表构造对每个 RHS 求值，
于是产物**只在目标运行时可加载**：在通用分析沙箱（luau CLI 等，
`Vector3`/`task` 为 nil）里**加载期即 `index nil` 崩溃**，任何解码都还没
开始。与 ⑱ 不同：**不需要用户传任何参数**，目标运行时自带这些 API，
天然「混淆后直接可运行」；只有非目标沙箱跑不了。

```bash
./target/release/luraph-rs --preset v15 --dialect luau --bind-env roblox in.lua out.lua
```

绑定态产物不进通用运行等价矩阵（CLI 跑不动是设计目的），只做语法校验 +
加载失败验证（`tests/env_bind_test.sh`，已内嵌 run_tests.sh）。
**诚实边界**：挡的是非目标运行时；若攻击方用 Roblox 环境（或桩了这些
API 的模拟器）运行则不拦，退化为 ⑲⑳ 结构防护 + 反调试。它是「抬门槛」，
与叠层其余层叠加。

同 `--seed` → 输出逐字节一致；不同 seed → 编码完全不同。  
分层开关：`--no-mangle` / `--no-minify` / `--no-strings` / `--no-flatten` /
`--no-junk` / `--no-numbers` / `--no-body` / `--no-antidbg` / `--vm`。

Luau 输入支持 `//`、`continue`、复合赋值、反引号插值、类型注解（解析期剥离）。

## 验证

```bash
cd luraph-rs
bash tests/run_tests.sh      # 官方矩阵：全语料 × 双方言 ×（high + vm + v15）
                             #   + ⑱ 绑定门控 10 项 + ㉑ 环境绑定 4 项 = 250 项
bash tests/run_presets.sh    # 预设 × 全语料（465 项）
bash tests/multiseed.sh      # VM/编码改动必跑（含绑定 5 种子扫描）
bash tests/bind_gate_test.sh # 增量⑱ 激活门独立跑法（已内嵌 run_tests.sh）
bash tests/env_bind_test.sh  # 增量㉑ 环境绑定独立跑法（已内嵌 run_tests.sh）
bash tests/bench_presets.sh  # 性能快照（见 docs/performance.md）
bash tests/gen_examples.sh   # 重生成 luraph-rs/examples/
```

当前（2026-09-08）：官方矩阵 **250/250**（含 ⑱ 绑定门控 10 项 + ㉑ 环境
绑定 4 项），预设矩阵 **465/465**，多种子回归 0 失败（含绑定扫描），
轮廓指纹 32/32，安全指纹 5/5。

## 性能（摘要）

完整表：`docs/performance.md`。数量级（Lua 5.1.5，`--seed 42`）：

- 混淆：非 VM 2–9 ms；VM ~15 ms
- `high` 运行约 4–16×；`vm` 小脚本约 20–100×，热表循环可到数百倍
- `vm` 容器有一份解释器骨架，再小的脚本也约 130 KB+

要帧时间就用 `high`；要 VM 护城河就用 `vm`/`max`。

## 约束

- 输出是 **5.1 兼容 Lua 源码**（无 goto / 位运算 / `//`）
- 共享语料/用户脚本若要双方言交叉一致：不要顶层新建全局（Luau CLI
  `luaL_sandbox` 只读）、不要依赖重复键表构造器的存储序
- 自包含脚本的流密码密钥**必然可恢复**——壁垒是每构建唯一 + 提取成本，
  不是密码学不可破（见 `docs/obfuscation-research.md` §2.2.1）

## 仓库

| 路径 | 内容 |
|---|---|
| `HANDOFF.md` | 零上下文交接（新会话/AI 先读） |
| `PROGRESS.md` | 里程碑进度 |
| `docs/implementation-plan.md` | L1–L7 清单 + 里程碑 + 更新日志 |
| `docs/performance.md` | 预设性能数据 |
| `docs/vm-l6-implementation.md` | VM 实现笔记（动 `vmgen/` 前必读） |
| `docs/obfuscation-research.md` | 混淆调研 + 双方言实测 |
| `docs/luraph15-analysis.md` | Luraph v15 样本分析 |
| `docs/luraph15-defense-analysis.md` | Luraph v15 防御手段全解析（安全导向） |
| `luraph-rs/` | Rust 本体（`src/vmgen/` = L6） |
| `luraph-rs/tests/` | 矩阵 / 预设 / 多种子 / 语料 |
| `luraph-rs/examples/` | 全语料混淆示例 |

当前里程碑：M0–M6 完成。
