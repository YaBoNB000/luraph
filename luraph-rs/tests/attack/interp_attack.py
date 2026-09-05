#!/usr/bin/env python3
"""S3/S4 攻击：解释器可见度 + 输出自描述度（静态，mangle 无关的量）。

S3 判定（指令语义可见）：寄存器索引形态 `[x + y]` 密度 + 4 参函数数
   ——解释器被加密后两者应骤降。
S4 判定（自描述消除）：大型数字阵列（≥100 词，字节码字表形态）/
   定形 LCG 生成器（(m*x+c)%2^28 三连）/ 大型 名→数 映射表（OC 形态）
   —— 三者联立 = 输出自描述。P3b 后：字节码字表拆槽+位置掩码、
   LCG 生成器变形（x*m+c + 分步局部名）、OC 表运行时生长（无名表）。
   AL/TK 解码表与样本同族刻意保留轮廓——内容层已由 P1/P2/P3a 加密，
   仅凭解码表无法重建程序（样本本身亦如此）。

用法: python3 interp_attack.py <obf.lua>
退出码: 0 = 攻击成功（解释器/素材可见）; 2 = 攻击失败。
"""
import re
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vmstatic


def main():
    path = sys.argv[1]
    src = open(path, encoding="utf-8").read()
    code = vmstatic.strip_strings(src)

    # S3: 解释器语义形态
    reg_pat = len(re.findall(r"\[\w+ ?\+ ?\w+\]", code))
    four_arg = len(re.findall(r"=function\(\w+,\w+,\w+,\w+\)", code))
    s3 = reg_pat >= 200 or four_arg >= 20

    # S4: 自描述素材
    bw = vmstatic.extract_bw(code)
    lcg = vmstatic.extract_lcg(code)
    alpha = vmstatic.extract_alphabet(code)
    tokens = vmstatic.extract_tokens(code)
    # 大型 名→数 映射表（OC 形态：>=20 个 名字=数字 连续项）
    oc_like = len(re.findall(r"\{\w+=\d+(?:,\w+=\d+){19,}\}", code))

    s4 = (len(bw) >= 100) and (lcg is not None) \
        and (alpha is not None) and (tokens is not None)

    print(f"S3 解释器可见度: 寄存器形态 {reg_pat} 处, "
          f"4 参函数 {four_arg} 个 -> {'FAIL（语义可见 → 缺点①）' if s3 else 'PASS'}")
    print(f"S4 自描述素材: 最大数字阵列 {len(bw)} 词 / LCG 定形生成器 "
          f"{'有' if lcg else '无'} / OC 形大表 {oc_like} 个 "
          f"(AL {'有' if alpha else '无'} / TK {'有' if tokens else '无'} 为轮廓保留项) -> "
          f"{'FAIL（输出自描述）' if s4 else 'PASS'}")
    return 0 if (s3 or s4) else 2


if __name__ == "__main__":
    sys.exit(main())
