# luraph

商业级 Lua 混淆器（对标 Luraph，Roblox 生态最强的商业混淆器）。

核心能力：**自定义 VM + 自有解释器**——用户 Lua 源码被编译为私有字节码，
由随输出脚本一起分发的「Lua 写的混淆解释器」执行。标准反编译器/格式化器
拿不到任何原生字节码或用户代码结构；每个构建的 VM（指令置换/派发树/
操作数编码/命名）都不同。

- **双目标方言**：Lua 5.1 与 Luau，同一管线产出，输出在两种解释器上
  行为逐字节一致（官方矩阵双向交叉验证）
- **混淆器本体**：Rust，std-only 零第三方依赖
- **多层纵深**：L1 名称混淆/压缩 → L2 字符串加密 → L3 控制流扁平化 →
  L4 数值拆分 → L5 整体加密容器 → **L6 私有字节码 VM** → L7 反篡改
  （金丝雀/校验和/错误行号重映射/时间陷阱）

## 快速开始

```bash
# 构建（工具链在仓库内 .tools/，沙箱外需自备 rustc/cargo ≥1.88）
cd luraph-rs
export PATH=/home/user/luraph/.tools/bin:$PATH   # 仓库内置工具链时
CARGO_NET_OFFLINE=true cargo build --release

# 混淆（5.1 目标）
./target/release/luraph-rs --dialect 5.1  --seed 42 in.lua out.lua
# 混淆（Luau 目标，支持 //、continue、复合赋值、反引号插值、类型注解）
./target/release/luraph-rs --dialect luau --seed 42 in.lua out.lua

# L6 VM 虚拟化（私有字节码 + 生成混淆解释器，~300KB 容器）
./target/release/luraph-rs --dialect 5.1 --vm --seed 42 in.lua out.vm.lua

# 分层开关：--no-mangle / --no-strings / --no-flatten / --no-junk /
#           --no-numbers / --no-body / --no-antidbg / --no-minify
# --seed 相同 → 输出逐字节一致；不同 → 编码完全不同
```

## 验证

```bash
cd luraph-rs
bash tests/run_tests.sh     # 官方矩阵：29 语料 × 双方言 ×（非 VM + VM）
                            # 当前 204 项全绿（含 5.1→luau 交叉 + 语法校验）
bash tests/multiseed.sh     # 多种子回归（VM/编码改动必跑）
bash tests/gen_examples.sh  # 重新生成 luraph-rs/examples/ 混淆示例
```

## 当前状态（2026-08-25）

- ✅ M0 地基 / M1 词法+字符串 / M2 控制流 / M3 数值+整体加密+反篡改
- ✅ M4 L6 VM（41 指令 + 每构建随机置换 + 生成的混淆解释器）+ 续期加固
  （29 语料暴露的 9 类语义 bug 全部修复，矩阵 204/204）
- ✅ M5 VM 完整随机面（SoA 平行数组 / 完整 7-bit / base-94+token /
  解码枢纽与状态元组随机 / 帧入场原语解包 / 反编译抽查）
- ⬜ M6 产品化（CLI 预设 + 产品文档 + 性能数据）

## 仓库地图

| 路径 | 内容 |
|---|---|
| `HANDOFF.md` | ★ 零上下文交接文档（新会话/AI 先读这个） |
| `PROGRESS.md` | 项目进度（每里程碑一节 + 目录结构） |
| `docs/implementation-plan.md` | 实施清单（L1–L7 逐项状态）+ 里程碑 + 更新日志 |
| `docs/vm-l6-implementation.md` | ★ VM 实现笔记（upvalue 单 cell 模型/9 类坑/调试工具箱） |
| `docs/obfuscation-research.md` | 混淆技术调研 + 双方言语义实测表 |
| `docs/luraph15-analysis.md` | Luraph v15.0 样本逆向分析报告 |
| `luraph-rs/` | Rust 混淆器本体（`src/vmgen/` = L6 VM 三件套） |
| `luraph-rs/tests/` | 官方矩阵 + 多种子回归 + 29 个测试语料 |
| `luraph-rs/examples/` | 全语料混淆示例（对照 `tests/cases/` 运行验证） |
| `lph/` | 早期 Lua 参考实现（存档，不复用） |
| `samples/` | Luraph v15.0 分析工件 |
