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
    # P1 起：常量区被密钥流掩码，完整字节行走会在首个 type-4 常量处
    # 失步——故判据改用「文档化定长头可识别且字段自洽」。P2 布局描述
    # 符化后此判据才失效。
    n_hdr = sum(1 for b in r["blobs"] if vmstatic.parse_header(b))
    ents = [entropy(b) for b in r["blobs"]]
    avg_ent = sum(ents) / len(ents) if ents else 0.0
    print(f"S2 攻击: blobs={len(r['blobs'])} 定长头可识别={n_hdr} "
          f"平均熵={avg_ent:.2f} bit/byte (加密底线≈8.0)")
    if n_hdr > 0:
        print("S2 FAIL（字节码头部仍为文档化规整结构 → 致命缺点② 存在）")
        return 0
    print("S2 PASS")
    return 2


if __name__ == "__main__":
    sys.exit(main())
