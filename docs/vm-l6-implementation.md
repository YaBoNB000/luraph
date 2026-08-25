# L6 VM 实现笔记（M4，2026-08-24）

> M4「VM 最小可用」落地记录：架构、语义模型、以及调试过程中踩出的
> **全部关键坑**（每个坑都有真实语料复现，修复后矩阵 136/136 全绿）。
> 后续 M5（随机面扩展）/ v2 改代码前必读本文件。

## 1. 总体架构

```
用户 Lua 源码
  → 现有管线（解析/去糖/symtab）
  → vmgen::compile()          AST → 私有字节码（每函数一个字节串）
  → vmgen::template::generate()  解释器 Lua 源码（每构建随机化）
  → 解释器源码过完整混淆管线（junk/mangle/flatten/strings/numbers/body/antidbg）
  → 输出 = [混淆过的解释器] + [加密字节码串] + 入口调用 VM(F1..Fn)
```

- **VM 函数值 = 原生 Lua 闭包**（makefn 产物）：pcall/coroutine/wrap/
  table.sort 等宿主特性天然可用，不做模拟（正确性优先，v15 同款取舍）。
- 每次调用：闭包包装器建 V 数组 → 共享 run 分派循环 → 结果 (out, n) 拆回。
- 寄存器 VM：13→9 字节定长指令 `[op u8][a,b,c,d u16×4]`（ISA 文档里的
  "13 bytes" 是旧注释，实际 9 字节；跳转目标 = 1 基字节偏移）。
- 常量池：nil/bool/number(十进制文本)/string；数字文本必须
  tonumber 可往返（整数→十进制，浮点→`{:?}`）。

### 操作数/槽位约定（极易错，统一在这里）

| 概念 | 编码 | 运行期 |
|---|---|---|
| 寄存器 r（0 基） | 操作数原值 | V[r+1] |
| 常量索引 k（0 基） | LoadK/Get/SetGlobal 的 b | C[k+1] |
| 函数索引 f（0 基） | Closure 的 b | PF[f+1] |
| upvalue 索引 u（0 基） | GetUp 的 b / SetUp 的 a | ups[u+1] |
| upvalue 描述符 | upsrc[u]（1 基宿主槽） | **bit15 = snapshot 标志**，低 15 位 = 创建帧 V 槽 |
| 跳转目标 | Jmp/Jf/Jt 的 b | 1 基字节偏移（9 字节/指令） |
| nres=255 | Call/CallM/CallE 的 c、Return 的 b | 可变结果（尾调用/return f()） |

### 多值调用协议（M4 最重灾区，四个调用 opcode）

- **Call** a,b,c,d：f=V[a+1]，参数 V[a+2..]，d=1 追加 varg；
  结果写 V[a+2..]（覆盖参数位），更新 lastbase/lastn。
- **CallE**（expand，"被展开的尾部调用"）：同 Call 布局，但结果**全量**
  展开，并把**结果计数**写回函数槽 V[a+1]（供上层 CallM/CallT 读）。
  d&1 = 追加 varg；d>=2 = 自身还有尾部展开调用（递归处理 f(g(h()))）。
- **CallM**（多尾外层）：f=V[a+1]，前 b 个固定参数 + 尾部
  `V[a+b+2]` 个展开参数（计数由前置 CallE 写入）；常规 nres 语义。
- **CallT**（表尾调用）：结果直接填表（计数推进），d = nfixed*2+tail。

**结果计数为什么必须用 select('#')**：Lua 表构造器 `{ f(...) }` 会
丢失**尾部 nil**（`{ nil }` 长度 0），`#` 随之失真。SENTINEL 方案
（`{ f(...), SENT }`）是**错的**——f 不再是最后一个元素，多值被
截断为 1 个（pcall 的第二个返回值会直接消失，表现为"函数返回了 table"）。
正确做法 `callcap`：

```lua
local function callcap(f, args, nargs)
  local w = function(...)
    local t = { ... }
    return t, select('#', ...)   -- 同一 vararg 列表：值 + 精确计数
  end
  return w(f(U(args, 1, nargs)))
end
```
写回寄存器时 `out[i]` 尾部取 nil 正好把槽清 nil，语义自洽。

## 2. upvalue 模型（两个正交机制，缺一不可）

> ⚠️ 本节描述 M4 初版模型（活引用+写回转发+快照）。2026-08-25 已换代为
> **单 cell 别名模型**（见 §8.1）：materialize 不再产生值副本。本节保留
> 仅作演进记录；现行代码以 §8.1 为准。

### 2.1 活引用（live cell）+ 写回转发

- 闭包创建时 `ups[i] = { v = 创建帧V, i = 槽 }`（活引用，调用期解引用）。
- **被引用函数在入场时把每个 upvalue 物化成本函数局部寄存器**
  （GetUp + declare）——这样：① 自身直接读写走寄存器；② 孙闭包的
  描述符能在父函数作用域里找到该符号（解析在"创建上下文"进行）。
- **写回转发（关键）**：物化寄存器只是副本。对物化 upvalue 的赋值
  必须同时 `SetUp(u, r)` 写回原槽，否则兄弟闭包/父帧看不到更新
  （counter 用例：`n = n + (step or 1)` 全变 0/旧值）。
  编译器实现：`declare()` 时若符号在 `upvals` 中，store_target 的
  Local 分支在 Move 之后补发 SetUp。

### 2.2 快照（snapshot cell）= 5.1 循环变量捕获语义

5.1/Luau 实测：`for i=1,3 do t[i]=function() return i end end`
→ **1 2 3**（每轮迭代产生新的循环变量；闭包绑定"创建时刻"的值）。
同理循环体内普通局部（`local q = i*7` 捕获）也是每轮新鲜。

- 编译器维护 `iter_syms`（在循环体作用域内 declare 的符号集合）+
  `loop_body_scopes` 计数（While/ForNum/ForGen/Repeat 体编译期 +1）。
- **描述符在创建上下文的 compile_function 里生成**：符号 ∈
  `iter_syms` → 描述符 |= 0x8000。
- 运行期 makefn：快照单元 `ups[i] = { snap = true, val = V[slot] }`
  （创建时刻拷贝），GetUp/SetUp 对 snap 单元读写 `val`。

**为什么活引用对"跨函数物化"仍然安全**：孙闭包捕获的是**中间函数帧**
里物化出的副本槽（不是循环帧的槽）；中间函数每次调用是新帧、新 V，
活引用指向"那次调用"的副本 —— 语义正确。只有**直接捕获循环帧槽**
的闭包才需要快照，而描述符恰好在循环帧上下文里生成，判定天然落点正确。

## 3. 双方言语义差异（VM 模板里必须处理的）

1. **Luau 冻结 `_G`**（实测）：`local G = _G; G[k] = v` 恒报
   "attempt to modify a readonly table"（新建/覆写都死）；但
   **直接全局赋值**（`k = v`）和 **`getfenv(0)`** 返回的表可写。
   模板统一 `local G = getfenv(0)`（5.1 下 getfenv(0) 就是 _G 本身，
   可写且同表）。
2. **`#` 元方法**：5.1 表**没有** `__len`（5.2+/Luau 有）。同一份
   模板要双宿主正确 → 运行期探测：
   `setmetatable({{}}, {__len=function() return 99 end})` 后 `#` 一次，
   `HAS_LEN_META = (结果==99)`。Len 分支按探测结果选元方法/原生 `#`。
   （注意 format! 模板字符串里花括号要双写；`{ {}}` 转义后才是 `{}`。）
3. 其余（`%` floor 语义、loadstring、字符串方法）双方言一致，
   无需分支（见 research §3 实测表）。

## 4. symtab 作用域修复（L1 混淆与 VM 模板的交互地雷）

VM 模板的分派是巨型 `if/elseif` 链，**每个分支声明同名局部**
（base/f/nfixed/args/...）。原 symtab 两处缺陷：

1. 分支**不推作用域**（Lua 语义：每个 then/elseif/else 块是新作用域）；
2. `lookup` 用 `Vec::find` 取**第一个**同名条目（同作用域重声明应
   **后者遮蔽前者**）。

后果（mangle 后必现）：第二分支的引用绑定到第一分支的符号，混淆改名后
变量张冠李戴 → 运行期 arithmetic-on-nil / 返回值变成 table。
修复：`lookup` 改为每作用域内**最后一次**声明胜出 + `Stmt::If`
各分支 new_scope/pop_scope。修复后非 VM 矩阵 68/68 仍全绿
（语料不踩该雷，VM 模板是第一个大规模踩雷者）。

## 5. 其他关键坑（一行一个）

- **密文串不得 UTF-8 透传**（输出里出现随机繁体/中文/阿拉伯字符的 bug）：
  `print_string_bytes` 对"合法可打印 UTF-8"做透传（用户串可读性设计），
  但密文/密钥流/字节码是任意字节——随机高字节恰好构成合法 UTF-8 时
  （如 E8 AF 9C = 嘱）会原样进字面量。修复：`Expr::Str` 增加
  `is_binary` 标志（body 密文块/strings L2 加密块+密钥常量/VM 字节码串
  = true）；binary 串一律 `\ddd` 全转义。minify 不再从解码字节重新编码
  （小密文块可能不含控制字节，启发式判二进制会漏）——改为 lexer 捕获
  原始字面量 span、minify 原样输出（字节级保真）。用户串（如 中文测试）
  在 L2 开启时本就加密；`--no-strings` 下仍可读透传（已验证）。
- **模板 format! 裸 `{}` = 位置占位符**：`run(..., {}, ...)` 会吞掉
  第一个具名参数（历史上把 `params="F1"` 塞进了 ups 参数位——主块无
  upvalue 所以"碰巧"能跑）。所有 Lua 字面量花括号必须 `{{ }}`。
- **LoadNil 模板 0/1 偏移**：`for i=0,b-1 do V[a+i]` 清错槽（清到
  前一个局部），应为 `for i=1,b do V[a+i]`。
- **repeat-until 条件作用域**：until 表达式**看得见** body 局部
   （5.1/Luau 一致）。编译器 Repeat 必须把 body 作用域在条件编译期
   保持打开（且 symtab 同语义：body 先 resolve、cond 后 resolve、
   作用域最后弹）。
- **`local x, fx = 7, function() return x end`**：fx 体内 x 是**全局**
  （x 的作用域从本语句之后才开始）——symtab 现状（values 先 resolve、
   后 declare）恰好正确，语料 `samestmt` 锁定该行为。
- **L7 时间陷阱阈值**：固定 5~15s 会误杀 VM 构建（300KB 容器解密+
  解释执行实测 ~11s CPU）。阈值改为 `rng.int(5,15) + 密文KB/10`
  （VM 容器 ≈ 35~45s；单步调试通常 >100x 慢，灵敏度不受影响）。
- **`f(g(h()))` 任意深度尾部展开**：CallE 递归（自身尾部也是调用时
  d|=2），布局：内层函数槽 = 外层固定参数区之后一格，计数写回内层
  函数槽，结果紧随其位。
- **`__call` 元方法**：调用一个带 `__call` 的表时，模板 `resolve_call`
  统一解析（函数表→`cc(f, args...)` 带 self；表表→`cc[f](args...)`
  不带 self，5.1 语义）。
- **for 循环变量寄存器**：每轮迭代 Move 到**同一**寄存器（编译期只
  分配一次），快照机制保证捕获语义——不要改成每轮 tmp()（描述符
  是编译期固定的单槽）。

## 6. 调试工具箱（本次实战沉淀）

- `LURAPH_VM_RAW=1`：输出未混淆的 VM 容器（隔离"VM 语义 bug" vs
  "混淆管线 bug"的第一手段）。
- `LURAPH_VM_TSRC=1`：dump 生成的解释器模板源码到 /tmp/vm_tsrc.lua
  （查 format!/花括号转义问题）。
- `LURAPH_VM_DBG=1`：compile_function 时打印 direct_up/nested_up/
  作用域快照（upvalue 描述符问题）。
- **Lua 反汇编器**（/tmp/disasm.lua，按 VM 文件 OC 表反查）：
  `lua51 disasm.lua out.lua [函数序号]` 打印 nregs/nups/upsrc/常量池/
  指令流（跳转目标是字节偏移）。VM 语义 bug 的定位 90% 靠它 +
  在模板分支里插 print（pc/寄存器值）。
- 二分 pass（--no-mangle/--no-strings/... 逐个关）定位管线交互 bug。

## 7. M5 剩余（VM 完整随机面）

已落地（M4 续期，2026-08-25 前）：
- [x] 随机决策树分派（2~4 层阈值分裂 + 平铺底部，`gen_dispatch_tree`）
- [x] 死指令填充（Nop 按 `code.len()/10` 注入随机位置，模板侧无害
      handler：`a+b` / `c*d` / `a*c+b` 三选一）
- [x] 7-bit 分块操作数编码（基础档：1/2-byte r16，破坏定长步长特征）
- [x] 操作数槽位随机（slot_perm：a/b/c/d 在 4 个流槽的每构建随机排列，
      编码器/解码器/模板共享）

剩余：
- [ ] SoA 平行数组容器（luraph14/15 同款）
- [ ] 7-bit 完整档（7/14/21-bit + 128 进制重建 + 2³² 归一化）
- [ ] 解码枢纽/状态元组位置每构建随机（当前模板结构固定，仅名称过
      mangle）
- [ ] base-N 编码 + token 转义（字节码串载体）
- [ ] 帧运行器入场原语解包随机化
- [ ] 反编译人工抽查（luac51 -l 输出应无用户结构可读）

## 8. M4 续期（2026-08-25）：单 cell 模型 + 应力测试踩坑

### 8.1 upvalue 模型换代：值副本 → 单 cell 别名

旧模型（§2）= 活引用 + 写回转发 + 快照。应力语料（stress_upvalues 的
`nested` 用例）暴露根本缺陷：**materialize = GetUp 值副本**，写回转发只
走一层 —— 孙层闭包的写不回传到规范 cell、跨层读拿陈旧值。5.1 的语义是
**每个 local 全程序只有一个 cell**，所有闭包引用同一 cell。

新模型（现行）：
- **Plain 描述符**（`slot`）：cell = `{ v = V, i = slot }`（创建帧的活
  槽位引用）。
- **Slot 描述符**（`0x8000 | slot`）：循环体局部/循环变量被闭包捕获时，
  每迭代在 `V[slot]` 建 cell 表 `{1 = value}`，makefn 绑定
  `{ v = V[slot], i = 1 }` —— 同一迭代的所有闭包 + 循环体自身读写同一
  cell（fresh per iteration，迭代间互不可见）。
- **Up 描述符**（`0xC000 | upidx`）：创建帧自己 materialize 了该符号时，
  闭包**直接别名父帧的 cell 对象**（`c[i] = upsf[upidx]`，upsf = 父帧
  cell 数组，需显式传入——makefn 词法作用域看不到 run 的局部）。
  materialize 从「GetUp 进新寄存器」变成**纯作用域别名**（不发射任何
  指令），全层共享规范 cell，读写天然同步。

模板 makefn 绑定序（先判 0xC000 再判 0x8000，互斥）：
```lua
if src >= 49152 then c[i] = upsf[src - 49152]
elseif src >= 32768 then c[i] = { v = V[src - 32768], i = 1 }
else c[i] = { v = V, i = src } end
```

### 8.2 应力语料暴露的其余 bug（一行一个）

- **CallT 表存储 off-by-one**：`for i=1,nout do t[n+i]=out[i]; n=n+1 end`
  的 n 在循环内自增 → 第 2 个结果落到 n+3。修：固定 n，结尾
  `V[c+1] = n + nout`。（`{9, m3(1,2,3)}` 原本 → `{9,1,nil,2}`）
- **GetTab 缺索引错误**：非 table 且无 metatable 静默 nil。修：
  `error('attempt to index a '..type(t)..' value', 0)`；有 mt 无 __index
  → nil（与 5.1 一致：5.1 非 table 本就不能设 metatable，该分支防御性
  保留）。
- **__index 表链**：`r = rawget(f, k)` 不跟随 f 自己的 __index 链
  （`leaf→mid→root` 双层 __index 表断链）→ 改原生 `f[k]`。
- **Assign 尾部展开后的 nil 填充**：`a, b = ..., x` 类边界公式错误时
  对已覆盖目标再写 nil（`k = next(t, k)` 必中：Call 正常返回 1 之后被
  `k = nil` 覆盖）。规则：尾值展开（npre < n）时 assigned = 全部目标。
- **`return ...` 只返回第一个 vararg** → Return 加 c=1 源选择（varargs）；
  **`local a,b = ...` / `a,b = ...`** 走 VarArgTab 取前缀（越界=nil）。
- **多余值必须「求值即弃」**：`local a = 1, 2, f()` 的 `2`/`f()` 仍要
  执行（副作用），只是不落目标。
- **5.1 构造器存储序**：luac -l 实测 SETLIST 在所有 SETTABLE 之后
  （`{10,20,name=..., [1]=11}` → 5.1 得 10，Luau 得 11）→ 编译器按
  `lua51` 标志延迟数组存储；Luau 源码序。重复键用例因此**不能进共享
  语料**（原始程序双方言输出本就不同，cross 必挂）——语料里已移除。
- **打印机后缀括号**：`Expr::Dot/Method/Index` 的 obj 必须用
  `Ctx::Suffix`（仅 Ident/Dot/Index/Call/Method 可裸写）——`(5).nope`
  此前打出 `5.nope`（双方言皆 malformed number）。
- **parser 多目标赋值**：`a, b = ...` 首目标后是逗号，原解析只认 `=`。
- **flatten 的 ForGen init**：单非调用迭代器**原样透传**（it = 该值）
  —— 5.1 裸 table 迭代器在运行时 call 报错（与宿主一致）；**不要**
  私自补 `next`（那是 Luau 专属，归一化在 parser 做）。
- **Luau `for k,v in t` 隐式 next**：语言级扩展（5.1 无）→ parser 在
  Luau 档把 `iters=[t]` 改写为 `iters=[next, t]`（全局 next，sym None）。

### 8.3 官方 luau CLI 沙箱（环境级，影响语料与用户程序）

`luaL_sandbox`（0.600+ 的 CLI 均调用）：全局表只读 + safeenv → 顶层
`newkey = v` = `attempt to modify a readonly table`。影响：
- 语料不得含顶层新建全局赋值（loops.lua 的 shadowtest 已改写）。
- VM 的 SetGlobal 天然同错（G = getfenv(0) 同一张只读表）→ 语义镜像
  正确，无需特殊处理。
- 重建 Luau 工具链时**必须**在自写 main 里复刻：luaL_openlibs +
  loadstring/collectgarbage 自定义全局 + require 文件 rehook +
  luaL_sandbox（漏掉 sandbox 会让语料出现假通过/假失败）。
