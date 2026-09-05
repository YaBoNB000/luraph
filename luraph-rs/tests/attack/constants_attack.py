#!/usr/bin/env python3
"""S1 攻击：常量恢复。判定 = 从静态脱壳出的常量里能对照源程序命中多少。

用法: python3 constants_attack.py <obf.lua> <src.lua>
退出码: 0 = 攻击成功（有常量被恢复）; 2 = 攻击失败（常量不可见）。
"""
import re
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vmstatic


def source_consts(src):
    """从源程序粗提字符串/数字字面量（去注释与字符串嵌套）。"""
    # 去注释
    src = re.sub(r"--\[\[.*?\]\]", "", src, flags=re.S)
    src = re.sub(r"--[^\n]*", "", src)
    strs = set()
    for m in re.finditer(r'"((?:[^"\\]|\\.)*)"|\'((?:[^\'\\]|\\.)*)\'', src):
        s = m.group(1) if m.group(1) is not None else m.group(2)
        strs.add(s)
    body = re.sub(r'"(?:[^"\\]|\\.)*"|\'(?:[^\'\\]|\\.)*\'', " ", src)
    nums = set()
    for m in re.finditer(r"(?<![\w.])(\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+)(?![\w.])", body):
        t = m.group(1)
        try:
            nums.add(float(t))
        except ValueError:
            pass
    return strs, nums


def main():
    obf, srcpath = sys.argv[1], sys.argv[2]
    r = vmstatic.attack(obf)
    if r["fail"]:
        print(f"S1 PASS (攻击失败于 {r['fail']})")
        return 2
    src = open(srcpath, encoding="utf-8").read()
    want_str, want_num = source_consts(src)

    got_str = {v.decode("utf-8", "replace") for t, v in r["consts"] if t == "str"}
    got_num = set()
    for t, v in r["consts"]:
        if t == "num":
            try:
                got_num.add(float(v.decode("ascii")))
            except ValueError:
                pass

    hit_str = {s for s in got_str if s in want_str and len(s) > 0}
    hit_num = {n for n in got_num if any(abs(n - w) < 1e-12 for w in want_num)}
    total = len(want_str) + len(want_num)
    hits = len(hit_str) + len(hit_num)
    print(f"S1 攻击: 脱壳常量 {len(r['consts'])} 条"
          f"（str {len(got_str)} / num {len(got_num)}）; "
          f"源常量 {total} 条, 命中 {hits} "
          f"(字符串 {sorted(hit_str)[:6]}… 数字 {sorted(hit_num)[:6]}…)")
    if hits > 0:
        print("S1 FAIL（常量被恢复 → 致命缺点③ 存在）")
        return 0
    print("S1 PASS")
    return 2


if __name__ == "__main__":
    sys.exit(main())
