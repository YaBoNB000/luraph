#!/usr/bin/env python3
"""安全指纹 S1–S6（「看着像+安全性也像」战役，P0 起）。

与 v15_fingerprint.py（轮廓指纹，32 条）并列：轮廓测「像不像」，
这里测「安不安全」。每项由对应攻击脚本判定——攻击成功 = 指纹红。

  S1 常量不可见        tests/attack/constants_attack.py
  S2 字节码不规则      tests/attack/structure_attack.py
  S3 指令语义不可见    tests/attack/interp_attack.py
  S4 自描述消除        tests/attack/interp_attack.py
  S5 动态加载防护      （P3 引入 load 后启用；当前 N/A）
  S6 运行等价          由官方矩阵/多种子承担（此处仅记录）

用法: python3 tests/security_fingerprint.py <obf.lua> <src.lua> [--quiet]
退出码: 0 = S1–S5 全绿; 1 = 有红项。

攻击脚本退出码约定: 0 = 攻击成功（指纹红）; 2 = 攻击失败（指纹绿）;
其他 = 攻击脚本自身错误（按红处理，防止崩溃被误判为安全）。
"""
import subprocess
import sys
import os

HERE = os.path.dirname(os.path.abspath(__file__))
ATK = os.path.join(HERE, "attack")

ITEMS = [
    ("S1", "常量不可见（脱壳恢复=0）", "constants_attack"),
    ("S2", "字节码不规则（规整布局不可解析）", "structure_attack"),
    ("S3", "指令语义不可见（解释器加密）", "interp_attack"),
    ("S4", "自描述消除（素材不可重建）", "interp_attack"),
    ("S5", "动态加载防护（load 原生复检）", None),
]


def run(script, *args):
    try:
        p = subprocess.run(
            [sys.executable, os.path.join(ATK, script), *args],
            capture_output=True, text=True, timeout=300)
        return p.returncode, p.stdout.strip()
    except Exception as e:  # noqa: BLE001
        return 99, f"攻击脚本异常: {e}"


def verdict(rc):
    """0 = 攻击成功 → 红; 2 = 攻击失败 → 绿; 其他 = 错误 → 红。"""
    if rc == 2:
        return True
    return False


def main():
    obf, srcpath = sys.argv[1], sys.argv[2]
    quiet = "--quiet" in sys.argv
    results = []

    rc, out = run("constants_attack.py", obf, srcpath)
    results.append(("S1", ITEMS[0][1], out, verdict(rc)))

    rc, out = run("structure_attack.py", obf)
    results.append(("S2", ITEMS[1][1], out, verdict(rc)))

    rc, out = run("interp_attack.py", obf)
    lines = out.splitlines()
    s3_line = next((l for l in lines if l.startswith("S3")), "")
    s4_line = next((l for l in lines if l.startswith("S4")), "")
    results.append(("S3", ITEMS[2][1], s3_line,
                    verdict(rc) if not s3_line else "PASS" in s3_line))
    results.append(("S4", ITEMS[3][1], s4_line,
                    verdict(rc) if not s4_line else "PASS" in s4_line))

    results.append(("S5", ITEMS[4][1], "N/A（P3 引入 load 后启用）", None))

    npass = sum(1 for *_, ok in results if ok)
    nfail = sum(1 for *_, ok in results if ok is False)
    for sid, desc, out, ok in results:
        mark = "✅" if ok else ("⬜ N/A" if ok is None else "❌")
        print(f"{sid} {mark} {desc}")
        if not quiet:
            for ln in out.splitlines():
                print(f"    {ln}")
    print(f"安全指纹: {npass}/5 通过, {nfail} 红, S6 运行等价见官方矩阵")
    return 1 if nfail else 0


if __name__ == "__main__":
    sys.exit(main())
