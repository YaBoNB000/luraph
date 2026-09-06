# Luau VM 混淆样本全量逆向还原报告

**分析对象**：
1. `real_string.v15.luau.lua.txt`（141 KB）
2. `real_ctrl.v15.luau.lua.txt`（138 KB）
3. `real_algo.v15.luau.lua.txt`（150 KB）

**混淆架构**：Luau VM Obfuscator（增量⑰：15 指令迷你 VM + 钥匙流折叠 + 动态加载 Handler + 反调试/防篡改守卫）  
**还原状态**：全量解密脱壳、原型（Prototype）字节码反编译完成，100% 语意与执行结果对齐。

---

## 一、 样本 1：`real_string.v15.luau.lua.txt`

### 1. 还原后的原始源代码 (Deobfuscated Source Code)

```lua
-- 凯撒加密（大小写字母移位）
local function caesar_cipher(str, shift)
    local res = {}
    for i = 1, #str do
        local c = string.byte(str, i)
        if c >= 65 and c <= 90 then
            c = (c - 65 + shift) % 26 + 65
        elseif c >= 97 and c <= 122 then
            c = (c - 97 + shift) % 26 + 97
        end
        res[i] = string.char(c)
    end
    return table.concat(res)
end

-- 统计字符串中的元音字母数量 (a, e, i, o, u, A, E, I, O, U)
local function count_vowels(str)
    local count = 0
    for i = 1, #str do
        local b = string.byte(str, i)
        if b == 97 or b == 101 or b == 105 or b == 111 or b == 117 or
           b == 65 or b == 69 or b == 73 or b == 79 or b == 85 then
            count = count + 1
        end
    end
    return count
end

-- 主执行逻辑
local text = "Hello, World!"
local encoded = caesar_cipher(text, 3)
print(encoded)
local vowels = count_vowels(text)
print("vowels:", vowels)
```

### 2. 核心算法与原型分析
* **Proto 1 (`caesar_cipher`)**：接收 `(str, shift)`，通过 `string.byte` 遍历 ASCII 码，针对大写 `[65, 90]` 与小写 `[97, 122]` 进行模 26 移位变换，最后通过 `table.concat` 拼装返回。
* **Proto 2 (`count_vowels`)**：接收 `(str)`，检查字符是否命中 10 个元音常量（`a, e, i, o, u, A, E, I, O, U`），累加计数并返回。
* **Proto 3 (Main)**：对 `"Hello, World!"` 执行位移 3 得到 `"Khoor, Zruog!"` 并打印，随后统计原字符串元音数 `3` 并打印。

### 3. 执行输出验证
```text
Khoor, Zruog!
vowels: 3
```

---

## 二、 样本 2：`real_ctrl.v15.luau.lua.txt`

### 1. 还原后的原始源代码 (Deobfuscated Source Code)

```lua
-- 递归斐波那契数列
local function fib(n)
    if n <= 1 then
        return n
    end
    return fib(n - 1) + fib(n - 2)
end

-- 试除法素数判定
local function is_prime(n)
    if n <= 1 then
        return false
    end
    for i = 2, n - 1 do
        if n % i == 0 then
            return false
        end
    end
    return true
end

-- 计算前 10 项斐波那契数之和
local sum = 0
for i = 1, 10 do
    sum = sum + fib(i)
end
print("fib sum:", sum)

-- 统计 1~10 之间的素数个数 (2, 3, 5, 7)
local prime_count = 0
for i = 1, 10 do
    if is_prime(i) then
        prime_count = prime_count + 1
    end
end
print("primes up to target:", prime_count)
```

### 2. 核心算法与原型分析
* **Proto 1 (`fib`)**：递归形式计算斐波那契数。
* **Proto 2 (`is_prime`)**：试除法判断素数，若 $n \le 1$ 返回 `false`，若存在 $i \in [2, n-1]$ 使得 $n \bmod i == 0$ 返回 `false`，否则返回 `true`。
* **Proto 3 (Main)**：
  - 循环 $i \in [1, 10]$ 累加 $\sum_{i=1}^{10} \text{fib}(i) = 1 + 1 + 2 + 3 + 5 + 8 + 13 + 21 + 34 + 55 = 143$。
  - 循环 $i \in [1, 10]$ 统计素数数量（2, 3, 5, 7 共 4 个）。

### 3. 执行输出验证
```text
fib sum: 143
primes up to target: 4
```

---

## 三、 样本 3：`real_algo.v15.luau.lua.txt`

### 1. 还原后的原始源代码 (Deobfuscated Source Code)

```lua
-- 线性同余发生器 (LCG) 生成长度为 n 的伪随机整数数组
local function generate_array(n)
    local t = {}
    for i = 1, n do
        t[i] = (i * 37 + 11) % 23
    end
    return t
end

-- 过滤出数组中的所有偶数
local function filter_evens(t)
    local evens = {}
    for i = 1, #t do
        if t[i] % 2 == 0 then
            table.insert(evens, t[i])
        end
    end
    return evens
end

-- 计算数组所有元素的平方和
local function sum_of_squares(t)
    local sum = 0
    for i = 1, #t do
        sum = sum + t[i] * t[i]
    end
    return sum
end

-- 查找数组中的最大值
local function find_max(t)
    local max_val = t[1]
    for i = 2, #t do
        if t[i] > max_val then
            max_val = t[i]
        end
    end
    return max_val
end

-- 主执行逻辑
local arr = generate_array(12)          -- 生成: {2, 16, 7, 21, 12, 3, 17, 8, 22, 13, 4, 18}
local evens = filter_evens(arr)         -- 过滤偶数: {2, 16, 12, 8, 22, 4, 18}
local sumsq = sum_of_squares(arr)
print("sumsq:", sumsq)
local even_sumsq = sum_of_squares(evens)
print("even count:", #evens, "even sumsq:", even_sumsq)
local max_val = find_max(arr)
print("max:", max_val)
```

### 2. 核心算法与原型分析
* **Proto 1 (`generate_array`)**：公式为 $t[i] = (i \times 37 + 11) \bmod 23$。当 $n = 12$ 时生成序列：
  `{2, 16, 7, 21, 12, 3, 17, 8, 22, 13, 4, 18}`。
* **Proto 2 (`filter_evens`)**：提取序列中模 2 为 0 的元素，得到 `{2, 16, 12, 8, 22, 4, 18}`（共 7 个偶数）。
* **Proto 3 (`sum_of_squares`)**：
  - 全数组平方和：$2^2 + 16^2 + 7^2 + 21^2 + 12^2 + 3^2 + 17^2 + 8^2 + 22^2 + 13^2 + 4^2 + 18^2 = 2249$。
  - 偶数子集平方和：$2^2 + 16^2 + 12^2 + 8^2 + 22^2 + 4^2 + 18^2 = 1292$。
* **Proto 4 (`find_max`)**：遍历数组寻找峰值，最大值为 $22$。
* **Proto 5 (Main)**：组合调用上述算法并格式化打印结果。

### 3. 执行输出验证
```text
sumsq: 2249
even count: 7 even sumsq: 1292
max: 22
```

---

## 四、 自动化脱壳与反编译一致性验证汇总

| 样本文件 | 核心逻辑 | 原样本执行输出 | 还原代码执行输出 | 校验结果 |
| :--- | :--- | :--- | :--- | :---: |
| `real_string.v15.luau.lua.txt` | 凯撒移位 + 元音统计 | `Khoor, Zruog!\nvowels: 3` | `Khoor, Zruog!\nvowels: 3` | **100% 一致** |
| `real_ctrl.v15.luau.lua.txt` | 斐波那契求和 + 素数计数 | `fib sum: 143\nprimes up to target: 4` | `fib sum: 143\nprimes up to target: 4` | **100% 一致** |
| `real_algo.v15.luau.lua.txt` | LCG 数组生成 + 偶数过滤 + 平方和 + 极值 | `sumsq: 2249\neven count: 7 ... max: 22` | `sumsq: 2249\neven count: 7 ... max: 22` | **100% 一致** |
