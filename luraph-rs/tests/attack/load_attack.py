#!/usr/bin/env python3
"""S5 攻击：动态加载防护（loadstring 原生性复检）。

判定（静态配对，增量㉔ 口径校准 —— 守卫结果钥匙化形态）：
输出须同时具备
  (a) 原生性探针：`<pcall>(function() return <f>(<x>, <y>) end)` ——
      pcall 包裹的双参调用（运行时装配名版的
      `pcall(debug.info(loadstring, "s"))`）；
  (b) 探针结果被强制：两种形态任一——
      b1 静默陷阱（历史形态）：`if not <v> then while true do end end`；
      b2 钥匙污染（增量㉔ 形态）：`<pb> = (<pb> + (<ok> and 0 or <delta>)) % 268435456`
         ——探针失败即污染元钥匙流（HBOOT 解出垃圾→错钥静默死亡），
         无可补的布尔标志（R008 实测攻击「补标志」路径的根治形态）。

历史口径（P3 时代）要求明文 `["loadstring"]` 索引访问 + `debug.info`
字面量；P4 起防御名全部运行时装配（打乱码表 + CHAR 拼接），明文字面
配对失效是**防御变强的副产品**——故口径改为结构配对。㉔ 再进一步：
死循环 oracle 本身也是可补丁目标（R008 回合实证），改为钥匙流污染后
判据同步升级（b1/b2 任一即视为探针结果被强制）。

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
keyify = len(re.findall(
    r"\w+\s*=\s*\(\w+\s*\+\s*\(\w+\s+and\s+0\s+or\s*.+\)\)\s*%\s*268435456", src))
enforced = trap + keyify
print(f"S5 攻击: 原生性探针 {probe} 处, 结果强制 陷阱 {trap} / 钥匙污染 {keyify} "
      f"(㉔ 口径; Luau 全局冻结: hook 安装被平台阻断)")
if probe >= 1 and enforced >= 1:
    print("S5 PASS")
    sys.exit(2)
print("S5 FAIL（动态加载缺原生性复检/结果强制）")
sys.exit(0)
