#!/usr/bin/env python3
"""S2 攻击：字节码结构规整性。判定 = 文档化定长布局能否完整解析 +
blob 信息熵。规整布局可解析 = 结构过于规整（致命缺点② 存在）。

用法: python3 structure_attack.py <obf.lua>
退出码: 0 = 攻击成功（结构规整）; 2 = 攻击失败（结构不规则）。
"""
import math
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vmstatic


def entropy(b):
    if not b:
        return 0.0
    cnt = [0] * 256
    for x in b:
        cnt[x] += 1
    n = len(b)
    return -sum(c / n * math.log2(c / n) for c in cnt if c)


def main():
    r = vmstatic.attack(sys.argv[1])
    if r["fail"]:
        print(f"S2 PASS (攻击失败于 {r['fail']})")
        return 2
    full = r["detail"].get("blob_full_parse", [])
    n_full = sum(1 for x in full if x)
    ents = [entropy(b) for b in r["blobs"]]
    avg_ent = sum(ents) / len(ents) if ents else 0.0
    print(f"S2 攻击: blobs={len(r['blobs'])} 完整按规整布局解析={n_full} "
          f"平均熵={avg_ent:.2f} bit/byte (加密底线≈8.0)")
    if n_full > 0:
        print("S2 FAIL（字节码为文档化规整结构 → 致命缺点② 存在）")
        return 0
    if avg_ent < 7.9:
        print("S2 FAIL（熵不足，疑似未加密/低熵结构）")
        return 0
    print("S2 PASS")
    return 2


if __name__ == "__main__":
    sys.exit(main())
