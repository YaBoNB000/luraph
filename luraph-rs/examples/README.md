# 混淆示例（所有常用语法）

`tests/cases/` 里每个语料文件经过 **L1 名称混淆 + L2 字符串加密**（当前已实现的
pass）后的输出。`*.5.1.lua` = 5.1 目标，`*.luau.lua` = Luau 目标（含 Luau 专属
语法：continue/复合赋值/`//`/反引号插值/类型注解）。

**对照方法**：`tests/cases/X.lua` 是原始代码，`X.5.1.lua` 是混淆结果——
两者在对应解释器上运行输出完全一致（`tests/run_tests.sh` 矩阵验证，62 项全绿）。

**重新生成**：

```bash
cd luraph-rs
/home/user/tools/bin/cargo build --release
tests/gen_examples.sh
```

## 当前已实现的混淆（M1）

- **L1 名称混淆**：所有局部变量/参数/循环变量/local function 名 → 随机名
  （短/中/长混合风格；避开关键字、程序用到的全局名；隐式 `self` 保持固定名）
- **L2 字符串加密**：
  - 所有字符串字面量 → 运行时解密调用 `dec(chunk1, chunk2, chunk3)`
  - 密码：加性密钥流 `enc[i] = (byte + key[i%24] + i) mod 256`（5.1 无位运算也能跑；
    双方言 `%` 均为 floor 语义——已实测）
  - 密钥拆成 3 段字面量 + 运行时展开为字节表（输出中无连续密钥）
  - 解密器/密钥表变量名同样随机化

## 尚未实现（M2–M6）

控制流扁平化（L3）、数值混淆（L4）、整体加密（L5）、自定义 VM（L6）、
反篡改（L7）——见 `docs/implementation-plan.md`。
