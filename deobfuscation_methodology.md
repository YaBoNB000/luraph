# Suda Luau VM 靶标：解混淆与脱壳流程

## 1. 范围、授权与目标

本流程针对用户拥有权利的 `D:\QQSave\target.luau.lua.txt`，用于评估自研 Luau VM 混淆器。目标是取得真实的 VM 原型、指令表示与常量池，并据此还原等价源码；不以单纯观察输出作为还原证据。

环境限制：标准 Luau CLI、全局表只读、无 `io`、无 `debug.sethook`，命令行参数不会传入脚本。策略因此采用“静态定位 + 工作副本局部插桩 + 动态验证”。

## 2. 交付物与结论概览

本轮产物：

- [还原源码](restored_source.luau)
- [VM 提取记录](vm_extraction.txt)
- [完整分析报告](analysis_report.md)

实际动态提取到：

```text
根原型：nparams=0，k={}，upsrc={}，无子原型
规范化 opcode tag：[11, 2, 42, 23, 10, 28]
常量：[3]=302907、[4]="print"、[6]="Suda"
```

由真实常量池和执行路径还原为：

```lua
print("Suda")
```

原靶标和还原版在 Luau CLI 下输出均为 `Suda`。

## 3. 阶段 A：建立干净运行基线（动态）

### 操作

使用官方 Luau CLI，不对原始靶标做修改：

```powershell
& .\work\luau\luau.exe D:\QQSave\target.luau.lua.txt
```

### 结果

```text
Suda
```

### 目的

这一步只确认样本可以运行、记录最终可见行为，并为后续插桩后的回归比较提供基线。此时**不**从输出推断源码。

## 4. 阶段 B：静态定位外层装配与数据汇合点（静态）

### 4.1 外层形态

样本为单行约 130 KB 的 Luau 源码，收束表达式为：

```lua
return setmetatable({...},{}):GB()(...);
```

外部 table 同时存放：原生 API、状态转移函数、算术辅助函数、密文片段、VM 引擎片段及调度器。大量函数仅根据状态返回下一状态，属于控制流平坦化。

### 4.2 反分析层

`GB` 内可见的检查包括：

- `newproxy` 和元表行为；
- `type`、`pcall`、`xpcall`、`rawget`、`rawset`、`getmetatable`、`setmetatable` 的一致性；
- `debug.info`、`loadstring`、`load` 的存在性/调用结果；
- `getfenv` 与 `print`/`warn` 的环境一致性；
- 失败后触发错误或不终止分支。

这类代码需要在插桩时保留原生 API，不宜通过粗暴替换全局函数来观察。

### 4.3 五段载荷装配点

静态定位到 `kH`：

```lua
local E=function(...)
    return C[6](C[1]..C[2]..C[3]..C[4]..C[5])
end
```

这里确认了五个分段经 `C[6]` 进入下一层。继续追踪状态机可见：

```lua
C[6]=GJ
```

因此 `GJ` 是运行时构建 VM 的关键闭包。

### 4.4 VM 原型装配点

在 `GJ` 尾部，静态代码可见：

```lua
local vU={}; local jC={};
return bL(qQ,jC,{},vU,0)
```

这里的 `qQ` 是反序列化/构建后即将传给解释器 `bL` 的根原型。它是最具价值的动态观察点。

此外，`mV` 包含：

```lua
local jX,wA,wL,ug,ol,SM=BH(yP,Gu,kC,Bf,vl)
```

`BH` 的返回值在 VM 初始化过程中被放入执行状态，是取得规范化指令列和常量表的第二个观察点。

## 5. 阶段 C：制作最小工作副本探针（动态）

### 原则

- 不修改原附件；
- 只在工作副本插入 `print` 和递归枚举函数；
- 不替换 `_G`、`debug`、`load`、`print` 或元表 API；
- 插桩后仍执行原始 VM 路径。

工作副本由 `work/make_probe.ps1` 生成，文件名为 `work/probe_payload.luau`。

### 探针 1：根原型转储

位置：`return bL(qQ,jC,{},vU,0)` 之前。

逻辑：递归遍历 `qQ`，以十六进制编码输出 string key/value，以避免二进制或不可见字符破坏记录。

目的：证明获得的是反序列化后的真实对象图，而不是猜测的业务逻辑。

### 探针 2：`BH` 返回值转储

位置：`mV` 中 `BH(yP,...)` 调用刚返回之后。

逻辑：对 `jX,wA,wL,ug,ol,SM` 逐项递归输出。

目的：取得 VM 实际初始化时使用的 opcode/操作数列和常量表。实测 `SM` 中出现明文 `print`、`Suda`。

### 探针 3：首次派发确认

位置：`Aa[1..4]` 调度函数完成装载之后，用透明包装函数记录 `pc` 与当前指令列。

目的：确认探针 1/2 得到的结构确实进入执行链。实测第一相位从 `pc=1` 与 opcode tag `11` 开始。

## 6. 阶段 D：运行探针并提取结构（动态）

执行：

```powershell
powershell.exe -ExecutionPolicy Bypass -File .\work\make_probe.ps1
& .\work\luau\luau.exe .\work\probe_payload.luau |
    Set-Content .\work\vm_capture.txt -Encoding utf8
```

### 6.1 根原型关键字段

实际结果：

```text
nparams = 0
n = 6
m = 6
S = [4, 6, 8]
k = {}
upsrc = {}
```

`k={}` 和 `upsrc={}` 表示没有子原型与上值。根对象还包含编码字段 `csb`、`a`、`ct`、`cn`、`csl`；完整数值摘录保存在 [vm_extraction.txt](vm_extraction.txt)。

### 6.2 规范化指令列

`BH` 返回的第一列为六个 opcode tag：

```text
[11, 2, 42, 23, 10, 28]
```

同时获得四列与之对齐的操作数：

```text
A = [14452, 26997, 28563, 1, 58156, 0]
B = [3, 32100, 40103, 1, 7, 0]
C = [936, 3089, 56983, 0, 37842, 0]
D = [5, 7, 30492, 1, 3, 0]
```

这些值属于**自定义 VM 格式**，不是标准 Luau 编译器 bytecode；因此没有把 tag 11、2、42 等伪装成标准 Luau opcode 名称。

### 6.3 常量池

`BH` 第六个返回对象中的非空常量槽：

```text
[3] number 302907
[4] string "print"
[6] string "Suda"
```

这就是源码还原的关键证据：目标调用的函数名和传入字符串均来自解码后数据结构。

## 7. 阶段 E：从产物到源码（静态解释 + 动态验证）

推导条件：

1. 根原型无参数、无上值、无子原型；
2. 常量池公开得到全局符号 `print` 和字符串 `Suda`；
3. VM 从根原型首个指令槽进入执行；
4. VM 实际输出 `Suda`；
5. 无其他字符串常量或子原型提供替代语义。

因此可读的等价源码是：

```lua
print("Suda")
```

这个还原来自常量池和 VM 执行对象，不是仅由输出文本反推。

## 8. 阶段 F：等价性验证（动态）

执行原版和还原版并比较标准输出：

```powershell
$original = & .\work\luau\luau.exe D:\QQSave\target.luau.lua.txt 2>&1
$restored = & .\work\luau\luau.exe .\outputs\restored_source.luau 2>&1
$original -ceq $restored
```

结果：`MATCH`，两者均为 `Suda`。

## 9. 证据强度与边界

| 结论 | 证据类型 | 置信度 |
|---|---|---|
| 原靶标输出 `Suda` | 原始样本直接运行 | 高 |
| 五段拼接、`C[6]=GJ`、`qQ→bL` | 可见层静态代码 | 高 |
| 根原型字段、无参数/无子原型 | `qQ` 运行时枚举 | 高 |
| 六条规范化指令槽及操作数 | `BH` 返回值运行时枚举 | 高 |
| `print`、`Suda` 常量 | `BH` 返回的明文常量表 | 高 |
| 等价源码 `print("Suda")` | 上述结构证据 + 输出逐字比较 | 高 |
| 每个自定义 opcode 的官方语义名 | 未取得 VM opcode 规范 | 未声明/不推测 |

## 10. 可复用流程模板

对同类 Lua/Luau 自定义 VM 目标，可按以下顺序复用：

1. 在干净解释器中记录基线输出与错误；
2. 找出载荷分段、拼接点、解密函数、反序列化点和最终 VM 调用点；
3. 优先在“原型传入 VM 前”转储对象，而非试图直接还原所有外层混淆；
4. 在 VM 初始化函数返回后转储其标准化结果，寻找常量池和 opcode 列；
5. 用透明包装确认对象真正被执行；
6. 只根据已提取的常量、原型和指令关系恢复源码；
7. 用独立运行结果验证，但不把运行输出作为唯一证据；
8. 保存探针、原始输出、提取物和版本 hash，确保审计可复现；
9. 清楚标注未解部分，尤其不要把自定义 opcode 强行映射成标准 Lua/Luau 指令。

## 11. 常见失败模式与处理

| 现象 | 可能原因 | 处理 |
|---|---|---|
| 插桩后卡死 | 触发反篡改或进入假路径 | 保留原生全局，缩小探针，插在闭包内部稳定点 |
| `load`/`loadstring` 不可用 | CLI/沙箱能力差异 | 不依赖自行加载代码，直接修改工作副本并由原装载器执行 |
| 二进制输出损坏 | 直接 `print` 原始载荷 | 对字符串键和值使用 hex/base64 编码 |
| 只看见加密表 | 转储位置早于反序列化 | 沿调用链向 `bL`/解释器入口移动 |
| 常量仍是编码值 | 观测在规范化前 | 在 VM 初始化/解码函数返回后再转储 |
| 只有输出，没有结构证据 | 将行为观察误当脱壳 | 必须补充原型、常量、指令列或装载产物 |

## 12. 复现文件说明

- `work/make_probe.ps1`：生成探针工作副本；
- `work/probe_payload.luau`：插桩后的临时运行文件；
- `work/vm_capture.txt`：探针原始输出；
- `outputs/vm_extraction.txt`：人工整理的可交付结构摘录；
- `outputs/restored_source.luau`：可独立运行的还原源码。

原始附件未被修改。
