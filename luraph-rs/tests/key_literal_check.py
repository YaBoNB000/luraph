#!/usr/bin/env python3
"""增量⑩ 门禁：钥匙字面量扫描。

用法:
  rm -f /tmp/keys.txt
  LURAPH_KEY_MANIFEST=/tmp/keys.txt luraph-rs ... in.lua out.lua
  python3 tests/key_literal_check.py out.lua /tmp/keys.txt [--quiet]

构建期每个密钥（LCG 种子/乘数/加数，均 >= 2^20）经
LURAPH_KEY_MANIFEST 落盘；本脚本逐一确认这些值没有以裸字面量形式
残留在输出里（钥匙必须只能由运行时碎片装配得到）。

退出码: 0 = 全部钥匙已去字面化; 1 = 有钥匙仍以裸字面量出现。
"""
import re
import sys


def main():
    out_path, mani = sys.argv[1], sys.argv[2]
    quiet = "--quiet" in sys.argv
    code = open(out_path, encoding="utf-8").read()
    bad, total = [], 0
    for line in open(mani, encoding="utf-8"):
        line = line.strip()
        if not line or "=" not in line:
            continue
        name, val = line.split("=", 1)
        total += 1
        # 裸字面量 = 完整数字 token（前后不得是数字/小数点/指数）
        if re.search(r"(?<![\d.])" + val + r"(?![\d.])", code):
            bad.append((name, val))
    if bad:
        for name, val in bad:
            print(f"  ❌ {name} = {val} 仍以裸字面量出现")
        print(f"钥匙扫描: {len(bad)}/{total} 泄漏 -> FAIL")
        return 1
    if not quiet:
        print(f"钥匙扫描: {total} 个钥匙全部去字面化 -> PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
