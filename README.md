# 纯静态 Lua / Luau 还原交付包

## 主要结果

业务载荷等价于 `print(12)`，前提是原包装器的宿主检查通过。
请先阅读 `lua_static_report.md`。其第 3 节列出 44 项具体依赖特征，第 2 节按“假设 → 操作 → 观察 → 结论”列出 14 个主要步骤。

## 安全／方法边界

- 原文件位于 `uploads/test.txt`，只作为数据读取。
- 没有执行原 Lua、解码出的 Lua、loadstring 或任何 VM 指令。
- `*.decoded.lua.txt` 是惯性文本提取物，不是建议执行的程序。
- `recovered_logic.lua` 是人工恢复的等价业务逻辑，不是逐字原始源码，且有意省略了保护层。
- 二进制文件是自定义序列化数据，不是原生 Lua 可执行字节码。
- Python 工具仅做词法提取、白名单算术 AST 折叠、字节解码、格式读取、静态 CFG 分析与反向编码比对。

## 目录

- `lua_static_report.md`：完整报告。
- `recovered/disassembly.md`、`bytecode.csv`、`prototype.json`：指令、常量、函数元信息。
- `recovered/recovered_logic.lua`：最小等价业务源码。
- `recovered/recovered_ir.lua.txt`：注释性、非可执行 IR。
- `recovered/stage1_loader.decoded.lua.txt`：2,686 字节片段加载器。
- `recovered/bytecode_parser.decoded.lua.txt`：9,848 字节格式解析器。
- `recovered/handlers/`：43 个原样提取的 handler 源文本。
- `recovered/all_handlers.annotated.md`：去重复声明并替换已知字符串后的可读版。
- `recovered/static_constants.json`、`vm_strings.json`：恢复的参数和字符串。
- `recovered/outer_cfg.json`：105 个外层路由叶子及正常路径。
- `recovered/segments_manifest.json`：44 个片段的编号、源槽、有效长度。
- `recovered/source_anchors.json`：原文件 0 起始字节偏移。
- `recovered/verification.json`：静态校验与哈希。
- `analysis/*.py`：本次辅助工具。

## 复现静态数据提取

使用 Python 3.10 或更新版本，只有标准库依赖。在包含本 README 的目录中依次执行：

```sh
python analysis/01_lex.py
python analysis/decode_segments.py
python analysis/parse_payload.py
python analysis/outer_cfg.py
python analysis/make_semantics.py
python analysis/verify_static.py
```

其中 `decode_segments.py` 调用的 `static_extract.py` 是我们自己的常量提取器，绝不是上传的 Lua 或已解出的加载器。

输出可包括长段解码源码文本；文本只被写入文件／显示，不会传入 Lua 解释器。没有隐藏的 VM 执行步骤。`arith()` 不接受函数调用、属性访问或任意 eval。

复现命令会重建格式化遮罩和数据提取文件，不会自动改写人工撰写的报告及业务重建文件。

## 一致性校验

- 194 字符外层载体字节和 = 16047。
- 四个操作数列之和 = 411152，与解析器嵌入的校验值一致。
- 44/44 片段有效长度部分的密文反向编码一致。
- 113 字节载荷、43 字节常量池、Base94 长度头及转义反向编码一致。

这些检查是静态格式／数据一致性验证，不是运行测试。
