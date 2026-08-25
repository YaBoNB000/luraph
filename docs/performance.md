# 性能数据（M6，2026-08-25）

> 环境：仓库内 Rust 1.88 release（`opt-level=3, lto=true`）+ Lua 5.1.5。
> 机器：沙箱单核级。数字是**一次** wall-clock，用来定数量级，不是正式基准。
> 命令：`luraph-rs/tests/bench_presets.sh`（`--seed 42`，5.1 目标）。

## 预设对照

| 语料 | 预设 | 混淆 ms | 输出字节 | 原脚本 ms | 混淆后 ms | 减速 |
|---|---|---:|---:|---:|---:|---:|
| `basics` | `low` | 2 | 1 154 | 1.8 | 1.6 | 0.9× |
| `basics` | `medium` | 2 | 5 759 | 1.8 | 1.8 | 1.0× |
| `basics` | `high` | 3 | 22 877 | 1.8 | 6.8 | 3.7× |
| `basics` | `vm` / `max` | 14 | 134 472 | 1.8 | ~40 | ~22× |
| `functions` | `low` | 2 | 2 720 | 2.3 | 2.2 | 0.9× |
| `functions` | `medium` | 6 | 23 381 | 2.3 | 19.0 | 8.1× |
| `functions` | `high` | 9 | 81 156 | 2.3 | 38.0 | 16× |
| `functions` | `vm` / `max` | 15–17 | 144 412 | 2.3 | ~223 | ~95× |
| `game_loop` | `low` | 2 | 2 663 | 1.9 | 1.8 | 0.9× |
| `game_loop` | `medium` | 5 | 19 882 | 1.9 | 2.8 | 1.5× |
| `game_loop` | `high` | 8 | 67 454 | 1.9 | 18.0 | 9.6× |
| `game_loop` | `vm` / `max` | 14 | 143 955 | 1.9 | ~47 | ~25× |
| `tables` | `low` | 2 | 2 598 | 1.7 | 1.7 | 1.0× |
| `tables` | `medium` | 4 | 9 683 | 1.7 | 2.3 | 1.3× |
| `tables` | `high` | 5 | 35 681 | 1.7 | 10.2 | 5.9× |
| `tables` | `vm` / `max` | 14 | 149 475 | 1.7 | ~48 | ~27× |
| `stress_bigtable` | `low` | 2 | 2 642 | 6.3 | 6.2 | 1.0× |
| `stress_bigtable` | `medium` | 3 | 12 097 | 6.3 | 49.8 | 7.9× |
| `stress_bigtable` | `high` | 5 | 42 040 | 6.3 | 62.7 | 9.9× |
| `stress_bigtable` | `vm` / `max` | 14–15 | 156 000 | 6.3 | ~3.8 s | ~600× |

`vm` 与 `max` 当前编码相同（v2 CPS/超级指令尚未落地），体积与运行时间在噪声内一致。

## 怎么读这些数

- **混淆本身很快**：非 VM 个位数毫秒；VM 约 15 ms（解释器模板生成 + 再过全套 pass）。
- **体积**：`low` 接近源码；`high` 因 L5 整段加密膨胀到数十 KB；`vm` 有一份 ~130 KB 的解释器骨架，小脚本也被抬到这个量级。
- **运行**：`low` 与原脚本同级；`medium`/`high` 主要吃 flatten + `loadstring` 解密；`vm` 是 Lua-on-Lua，热路径上的表/循环会到两个数量级（`stress_bigtable` 的 600× 是上界参考，不是典型游戏脚本）。
- **选型**：要可读性对抗、在乎帧时间 → `high`；要标准反编译器彻底失效 → `vm`/`max`。

重跑：

```bash
export PATH=/home/user/luraph/.tools/bin:$PATH
cd luraph-rs && CARGO_NET_OFFLINE=true cargo build --release
bash tests/bench_presets.sh
```
