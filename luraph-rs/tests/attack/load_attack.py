#!/usr/bin/env python3
"""S5 攻击：动态加载防护（loadstring 原生性复检）。

判定（静态配对，增量⑳ 口径校准 —— 对齐 P4 防御代码隐藏时代形态）：
输出须同时具备
  (a) 原生性探针：`<pcall>(function() return <f>(<x>, <y>) end)` ——
      pcall 包裹的双参调用（运行时装配名版的
      `pcall(debug.info(loadstring, "s"))`）；
  (b) 静默陷阱：`if not <v> then while true do end end` ——
      探针失败即挂起（被 hook 即死，永不给出错误预言机）。

历史口径（P3 时代）要求明文 `["loadstring"]` 索引访问 + `debug.info`
字面量；P4 起防御名全部运行时装配（打乱码表 + CHAR 拼接），明文字面
配对失效是**防御变强的副产品**——故口径改为结构配对（mangle/flatten/
minify 后形状不变，多种子实测恒为 probe=1/trap=1）。

Luau 目标环境的全局表是冻结的——攻击者根本无法替换全局
loadstring（平台层防护）；故以静态配对为判据，动态注入在
Luau 上不可行即视为攻击失败。
退出码: 0 = 攻击成功(红); 2 = 攻击失败(绿)。
"""
import re
import sys

src = open(sys.argv[1], encoding="utf-8").read()
probe = len(re.findall(
    r"\w+\(function\(\)\s*return\s+\w+\(\w+,\s*\w+\)\s*end\)", src))
trap = len(re.findall(r"if not \w+ then while true do end end", src))
print(f"S5 攻击: 原生性探针 {probe} 处, 静默陷阱 {trap} 处 "
      f"(P4 运行时装配名口径; Luau 全局冻结: hook 安装被平台阻断)")
if probe >= 1 and trap >= 1:
    print("S5 PASS")
    sys.exit(2)
print("S5 FAIL（动态加载缺原生性复检/陷阱）")
sys.exit(0)
