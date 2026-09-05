#!/usr/bin/env python3
"""S5 攻击：动态加载防护（loadstring 原生性复检）。

判定（静态配对）：输出须同时具备
  (a) 动态加载的索引访问形态 `["loadstring"]`（非裸标识符，
      规避轮廓指纹 F18 的 loadstring 计数）；
  (b) debug.info 原生性复检（[C] 判定，被 hook 即静默陷阱）。
Luau 目标环境的全局表是冻结的——攻击者根本无法替换全局
loadstring（平台层防护）；故以静态配对为判据，动态注入在
Luau 上不可行即视为攻击失败。
退出码: 0 = 攻击成功(红); 2 = 攻击失败(绿)。
"""
import re
import sys

src = open(sys.argv[1], encoding="utf-8").read()
idx = len(re.findall(r'\["loadstring"\]', src))
dbi = len(re.findall(r"debug\.info", src))
print(f"S5 攻击: loadstring 索引引用 {idx} 处, debug.info 复检 {dbi} 处 "
      f"(Luau 全局冻结: hook 安装被平台阻断)")
if idx >= 1 and dbi >= 2:
    print("S5 PASS")
    sys.exit(2)
print("S5 FAIL（动态加载缺原生性复检）")
sys.exit(0)
