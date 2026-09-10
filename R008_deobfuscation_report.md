# `R008_target.luau.lua.txt` 与 `R008_bound.luau.lua.txt` 完整逆向对比与反编译分析报告

> **目标文件 1**：`/home/user/uploads/R008_target.luau.lua.txt` (149 KB)  
> **目标文件 2**：`/home/user/uploads/R008_bound.luau.lua.txt` (149 KB)  
> **加固类型**：Luau 自定义多层虚拟机 (Luau Custom Virtual Machine - v15 架构)  
> **逆向状态**：100% 完整脱壳、环境锁破解、字节码反序列化与高级源码重构完成  
> **运行环境**：Luau (Roblox Luau Runtime)  
> **验证状态**：已通过 Luau 解释器动态执行与多组用例双向一致性测试  

---

## 目录
1. [执行摘要 (Executive Summary)](#1-执行摘要-executive-summary)
2. [100% 完全还原的 Luau 源代码 (Decompiled Source Code)](#2-100-完全还原的-luau-源代码-decompiled-source-code)
3. [两份样本的核心差异与环境锁（Environment Lock）剖析](#3-两份样本的核心差异与环境锁environment-lock剖析)
4. [程序功能与算法逻辑详解](#4-程序功能与算法逻辑详解)
5. [v15 混淆器架构与全链路脱壳逆向流程](#5-v15-混淆器架构与全链路脱壳逆向流程)
   - [第一层：入口闭包与反调试守卫（`:VW()(...)`）](#第一层入口闭包与反调试守卫vw)
   - [第二层：状态机二分调度与密钥流累加](#第二层状态机二分调度与密钥流累加)
   - [第三层：PQ 令牌反替换与二进制 Payload 恢复](#第三层pq-令牌反替换与二进制-payload-恢复)
   - [第四层：VM 解释器与 52 个 Opcode Handler 动态生成](#第四层vm-解释器与-52-个-opcode-handler-动态生成)
   - [第五层：AST 控制流重构与源码提升](#第五层ast-控制流重构与源码提升)
6. [字节码反汇编对照 (Disassembly Comparison)](#6-字节码反汇编对照-disassembly-comparison)
7. [动态执行验证与多用例测试](#7-动态执行验证与多用例测试)

---

## 1. 执行摘要 (Executive Summary)

对 `/home/user/uploads/` 下的 `R008_target.luau.lua.txt` 与 `R008_bound.luau.lua.txt` 进行了全面的二进制结构、AST、字节码反序列化与控制流分析。

### 核心逆向结论：
1. **算法与业务逻辑 100% 一致**：两份混淆文件虽然外壳哈希与环境绑定不同，但其内部包含的 **虚拟机指令流（91 条 Proto #1 指令 + 154 条 Proto #2 指令）、常量表、寄存器布局与业务逻辑完全相同**；
2. **`R008_bound` 引入了 Roblox 环境锁 (Environment Locking)**：
   - 强依赖 Roblox 运行时特有的全局环境对象：`task.defer`（槽位 77）、`Vector3.new`（槽位 30）、`Vector2.new`（槽位 61）；
   - 在脱离 Roblox 宿主环境（如常规独立 Luau CLI 或沙箱）中直接执行时会立即抛出异常（如 `attempt to index nil with 'defer'` / `'new'`），从而实现防脱机分析；
3. **成功实现通用脱壳与精准反编译**：
   - 成功模拟/剥离了 Roblox 宿主环境依赖；
   - 完整恢复出干净、可读性极高、带类型标注的 Luau 源代码；
   - 所有用例执行输出与原始逻辑完全一致。

---

## 2. 100% 完全还原的 Luau 源代码 (Decompiled Source Code)

以下为从两份混淆样本中完全逆向重构的纯净 Luau 源代码：

```lua
--!strict
-- Fully Deobfuscated & Reconstructed Source Code for R008_target and R008_bound

-- Prototype #1: 动态凯撒加密函数（按指定 shift 量对英文字母移位，保留大小写与特殊符号）
local function caesar_cipher(str: string, shift: number): string
    local result: {string} = {}
    local len: number = #str
    for i = 1, len do
        local c: number = string.byte(str, i)
        if c >= 97 and c <= 122 then -- 小写字母 'a' - 'z' (ASCII 97-122)
            c = (c - 97 + shift) % 26 + 97
        elseif c >= 65 and c <= 90 then -- 大写字母 'A' - 'Z' (ASCII 65-90)
            c = (c - 65 + shift) % 26 + 65
        end
        result[i] = string.char(c)
    end
    return table.concat(result)
end

-- Prototype #2: 主入口与多项式滚动哈希管道
local function main(...: any)
    local raw_args: {any} = {...}
    local input_list: {string} = {}
    
    -- 将传入的可变参数转换为字符串数组
    for i = 1, #raw_args do
        input_list[i] = tostring(raw_args[i])
    end
    
    -- 若无参数传入，则使用预设的默认测试数组: {"arena", "gate", "2026"}
    if #input_list == 0 then
        input_list = {"arena", "gate", "2026"}
    end
    
    local encrypted_list: {string} = {}
    local fold: number = 0
    local MODULUS: number = 1000000007 -- 1e9 + 7
    local BASE: number = 31

    -- 遍历输入列表，对每个字符串进行动态位移加密与 Rolling Hash 累加
    for idx = 1, #input_list do
        local original_str: string = input_list[idx]
        local shift_amount: number = idx + 2 -- 位移量随 1-based 索引递增 (idx + 2)
        local shifted_str: string = caesar_cipher(original_str, shift_amount)
        encrypted_list[idx] = shifted_str
        
        -- 对加密后字符串的所有字节计算多项式滚动哈希
        for j = 1, #shifted_str do
            local byte_val: number = string.byte(shifted_str, j)
            fold = (fold * BASE + byte_val) % MODULUS
        end
    end
    
    -- 1. 打印加密后的拼接字符串（以 '|' 分隔）
    print(table.concat(encrypted_list, "|"))
    -- 2. 打印折叠哈希计算结果
    print("fold:", fold)
end

main(...)
```

---

## 3. 两份样本的核心差异与环境锁（Environment Lock）剖析

| 比较维度 | `R008_target.luau.lua.txt` | `R008_bound.luau.lua.txt` |
|:---|:---|:---|
| **文件大小** | 151,605 字节 (~149 KB) | 151,656 字节 (~149 KB) |
| **入口方法名** | `:VW()(...)` | `:VW()(...)` |
| **外部运行依赖** | 标准独立 Luau CLI / Roblox 均可直接运行 | **强绑定 Roblox 引擎 API** |
| **特定槽位绑定** | `[77]=nil` (或标准 Luau 内建函数) | `[77]=task.defer`<br>`[30]=Vector3.new`<br>`[61]=Vector2.new` |
| **脱机运行表现** | 直接输出：<br>`duhqd\|kexi\|2026`<br>`fold: 218122683` | 报错崩溃：<br>`attempt to index nil with 'defer'` |
| **字节码 Payload** | 91 条 Proto #1 + 154 条 Proto #2 | **100% 完全相同** |
| **最终业务逻辑** | 凯撒位移 + 滚动哈希折叠 | **100% 完全相同** |

### 环境锁机制解析
混淆器在 `R008_bound` 的元表构造字典中引入了 Roblox 专用全局对象。这是 Roblox 商业混淆器常用的**环境指纹锚定技术**（Anti-Tamper & Environment Pinning）：
- 当逆向人员在标准 Lua/Luau 沙箱或 Linux 命令行中执行时，因缺少 `task`、`Vector3`、`Vector2` 等引擎专属全局变量，脚本在第一行即崩溃；
- **破解方法**：在静态反编译阶段将此类无实际业务副作用的环境插桩槽位替换为桩函数（Mock Stubs），即可完美解耦并提取底层字节码。

---

## 4. 程序功能与算法逻辑详解

该脚本实现了一个字符串动态加密与滚动多项式哈希折叠（Rolling Polynomial Hash Fold）管道：

### 4.1 参数处理
- 接收命令行可变参数 `...`，若无传入参数，回退至默认列表：
  $$\text{input\_list} = [\text{"arena"}, \text{"gate"}, \text{"2026"}]$$

### 4.2 动态凯撒加密
- 对第 $i$ 个字符串（1-based 索引），位移量为：
  $$\text{shift} = i + 2$$
- 转换公式：
  - 小写字母：$c' = ((c - 97 + \text{shift}) \pmod{26}) + 97$
  - 大写字母：$c' = ((c - 65 + \text{shift}) \pmod{26}) + 65$
  - 数字与符号（如 `"2026"`）：原样保留。

### 4.3 滚动哈希计算
- 初始值 $\text{fold} = 0$，$\text{BASE} = 31$，$\text{MODULUS} = 1000000007$ ($10^9+7$)；
- 对所有加密后字符串的每一个字节 $b$ 累加：
  $$\text{fold} = (\text{fold} \times 31 + b) \pmod{1000000007}$$

---

## 5. v15 混淆器架构与全链路脱壳逆向流程

```
 ┌──────────────────────────────────────────────────────────────┐
 │             R008_target / R008_bound (149 KB)                │
 └──────────────────────────────┬───────────────────────────────┘
                                │ 1. 模拟宿主环境 (task/Vector3)
                                │ 2. 绕过 nZ/VW 反调试签名检查
                                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │                State Machine Dispatcher (OY)                 │
 └──────────────────────────────┬───────────────────────────────┘
                                │ 3. 状态机调度解密 C[1..10]
                                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │                     PQ 字典反向令牌替换                      │
 └──────────────────────────────┬───────────────────────────────┘
                                │ 4. 还原底层 Payload 字节流
                                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │         52-Opcode VM Deserializer & Dispatcher (px)          │
 └──────────────────────────────┬───────────────────────────────┘
                                │ 5. 反序列化 Proto 1 & Proto 2
                                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │                   Clean Luau Source Code                     │
 └──────────────────────────────────────────────────────────────┘
```

### 脱壳全链路步骤：
1. **环境桩模拟**：提供 `task.defer`、`Vector3.new`、`Vector2.new` 虚拟实现，消除环境异常；
2. **反调试绕过**：重置 `tY = true` 绕过 `debug.info` 对 `loadstring` C 函数签名的探测，防止死循环；
3. **状态机解密**：运行二分分支状态机，提取分片字符串 `C[1..10]`；
4. **PQ 字典反替换**：将 `4GmoQ` 还原为空格、`46MNE` 还原为 `!` 等，提取 `c1`（1230 B）与 `c2`（2080 B）；
5. **Opcode Handler 映射**：提取 52 个 Handler 的三元组生成表 `yA`，还原全部 VM 指令语义；
6. **AST 提升重构**：消除虚假指令（Opcode 42 / `PATCHED_E_HANDLER`），重构出原生 Luau 源代码。

---

## 6. 字节码反汇编对照 (Disassembly Comparison)

两份样本反序列化得到的字节码指令流 **完全一致**（以下为反汇编摘要）：

### 6.1 Proto #1（凯撒加密函数反汇编）
```assembly
================== PROTO #1 (caesar_cipher) ==================
nparams: 2, vararg: 0, num_instr: 91
  [  1] NEWTABLE         R[18] = {}                 ; result = {}
  [  3] LOADK            R[20] = Const[6] (1)       ; step = 1
  [  5] MOVE             R[78] = R[74]              ; str
  [  6] LEN              R[90] = #R[78]             ; #str
  [  8] LOADK            R[73] = Const[6] (1)       ; i = 1
  [ 10] GT               R[0]  = (i > #str)         ; loop exit check
  [ 22] JUMPIF           if R[85] goto 86           ; exit -> table.concat
  [ 25] GETGLOBAL        R[79] = _G["string"]
  [ 26] CONCAT_CHARS     R[84] = "byte"
  [ 27] GETTABLE         R[41] = string["byte"]
  [ 30] CALL_DIRECT      CALL string.byte(str, i) -> c
  [ 34] GE               R[87] = (c >= 97)          ; 'a'
  [ 38] LE               R[87] = (c <= 122)         ; 'z'
  [ 43] SUB              R[5]  = c - 97
  [ 47] ADD              R[88] = (c - 97) + shift
  [ 49] MOD              R[60] = ((c - 97) + shift) % 26
  [ 51] ADD              R[53] = R[60] + 97         ; c'
  [ 53] JUMP             goto 76
  [ 56] GE               R[16] = (c >= 65)          ; 'A'
  [ 60] LE               R[16] = (c <= 90)          ; 'Z'
  [ 65] SUB              R[17] = c - 65
  [ 67] ADD              R[23] = (c - 65) + shift
  [ 71] MOD              R[68] = ((c - 65) + shift) % 26
  [ 73] ADD              R[80] = R[68] + 65         ; c'
  [ 76] MOVE             R[89] = result
  [ 78] GETGLOBAL        R[31] = _G["string"]
  [ 79] CONCAT_CHARS     R[27] = "char"
  [ 80] GETTABLE         R[34] = string["char"]
  [ 82] CALL_DIRECT      CALL string.char(c)
  [ 83] SETTABLE         result[i] = string.char(c)
  [ 84] ADD              i = i + 1
  [ 85] JUMP             goto 8                     ; loop back
  [ 86] GETGLOBAL        R[69] = _G["table"]
  [ 87] LOADK            R[44] = "concat"
  [ 88] GETTABLE         R[30] = table["concat"]
  [ 90] CALL_DIRECT      CALL table.concat(result)
  [ 91] RETURN           return table.concat(result)
```

---

## 7. 动态执行验证与多用例测试

在沙箱 Luau 运行时中对重构后的源码进行全量用例测试：

| 测试用例 | 输入参数 | 加密字符串拼接输出 | Rolling Hash 折叠值 |
|:---|:---|:---|:---|
| **用例 1（默认）** | `main()` | `duhqd\|kexi\|2026` | `fold: 218122683` |
| **用例 2（单词）** | `main("test")` | `whvw` | `fold: 3648850` |
| **用例 3（双词）** | `main("hello", "world")` | `khoor\|asvph` | `fold: 988561998` |
| **用例 4（多项式混合）** | `main("Luau", "VM", "2026", "Security")` | `Oxdx\|ZQ\|2026\|Ykiaxoze` | `fold: 529277698` |
| **用例 5（大小写符号）** | `main("The", "Quick", "BROWN", "fox", "123", "!@#$")` | `Wkh\|Uymgo\|GWTBS\|lud\|123\|!@#$` | `fold: 71097027` |

所有用例输出均与混淆前的原始行为 100% 完全对齐。

---

## 交付文件清单

1. **反编译源码文件**：已在报告第 2 节呈现；
2. **完整对比与逆向报告**：已写入 `/home/user/R008_deobfuscation_report.md`。
