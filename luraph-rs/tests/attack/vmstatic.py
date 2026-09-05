#!/usr/bin/env python3
"""vmstatic — v15 输出的静态白盒脱壳链（安全指纹 S1/S2 的攻击基座）。

攻击模型：分析者「懂格式、无种子、不运行程序」。全部素材取自输出文本：
  1. BW 大数字数组（u32 字表，建议5 存储）→ 最大的 {...数字...} 字面量
  2. LCG 密钥流常数（kg 槽生成器函数体里的三段 (m*x+c)%268435456）
  3. ks 种子（ks 槽的纯数字字段）
  4. chunk 边界（staging handler 的 for wi=sw,ew / sub(...,1,blen) / C[idx]）
     —— handler 的先后次序从 CPS 循环叶子的调用序列恢复（模块表字段被
     洗牌过，不能按字段出现顺序）
  5. 每 chunk 重放 3 步 LCG → key → 解 XOR → carrier 串
  6. AL/TK 表从解释器文本提取（byte→0..93 索引表 + 5 字符 token 表）
  7. base-94 5→4 解码 + token 反转义 → 原始字节码 blob
  8. 按规整布局解析 blob（定长头 + 常量表 + SoA）→ 明文常量

这是「输出自描述」的完整证明：每一步的素材都在静态文本里。
"""
import re
from collections import defaultdict

MOD = 268435456
SPECIALS = [34, 39, 37, 32, 36, 33, 126, 35, 125, 38]


def strip_strings(src):
    """把字符串/长字符串内容替换为空格（保留引号位置），防正则误配。"""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        ch = src[i]
        if ch == "[" and re.match(r"\[(=*)\[", src[i:i + 12]):
            lvl = re.match(r"\[(=*)\[", src[i:i + 12]).group(1)
            closer = "]" + lvl + "]"
            j = src.find(closer, i + 2 + len(lvl))
            j = n if j < 0 else j + len(closer)
            for k in range(i, j):
                out[k] = " "
            i = j
        elif ch in "\"'":
            q = ch
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == q:
                    break
                j += 1
            for k in range(i + 1, min(j, n)):
                out[k] = " "
            i = j + 1
        elif src.startswith("--", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def extract_bw(code):
    """最大的纯数字数组字面量 = BW u32 字表。"""
    best = []
    for m in re.finditer(r"\{(\d+(?:,\d+)+)\}", code):
        nums = [int(x) for x in m.group(1).split(",")]
        if len(nums) > len(best):
            best = nums
    return best


def extract_lcg(code):
    """kg 生成器：函数体内恰有 3 段 (m*x+c)%268435456 + local x=b[ks]。

    返回 (ks_slot, [(m1,c1),(m2,c2),(m3,c3)])。minified 输出里函数体以
    `function(b) ... end` 出现；用括号配平切函数体。
    """
    for m in re.finditer(r"function\(b\)", code):
        # 括号配平找函数体
        depth, i = 0, m.end()
        # 找 function 体的配对 end 不可靠（minified 无换行），改用
        # 平衡扫描到第一个 `%268435456` 三连出现为止的窗口
        seg = code[m.start():m.start() + 400]
        trips = re.findall(r"\((\d+)\*x\+(\d+)\)%268435456", seg)
        if len(trips) == 3:
            ks = re.search(r"local x=b\[(\d+)\]", seg)
            if ks:
                return int(ks.group(1)), [(int(a), int(b)) for a, b in trips]
    return None


def extract_ks_seed(code, ks_slot):
    """模块表里 [ks_slot]=<纯数字>（函数值的不算）。"""
    pat = re.compile(r"\[%d\]=(\d+)(?![\w.(])" % ks_slot)
    for m in pat.finditer(code):
        # 排除紧接函数/表的情况：纯数字后面是 , 或 }
        nxt = code[m.end():m.end() + 1]
        if nxt in ",}":
            return int(m.group(1))
    return None


def extract_chunk_handlers(code):
    """直接定位每个 `for wi=` 出现（= 一个存储 handler），前向窗口取
    它自己的边界参数；handler 的存储位置由 `C[idx]` 的 idx 编码，
    无需恢复叶子次序。返回 [(sw, ew, blen, idx)]。"""
    out = []
    for m in re.finditer(r"for wi=(\d+),(\d+) do", code):
        seg = code[m.end():m.end() + 900]
        bl = re.search(r"sub\(table\.concat\(t\),1,(\d+)\)", seg)
        ix = re.search(r"C\[(\d+)\]=table\.concat\(o\)", seg)
        if bl and ix:
            out.append((int(m.group(1)), int(m.group(2)),
                        int(bl.group(1)), int(ix.group(1))))
    return out


def extract_alphabet(code):
    """找具名局部表：>=94 条形如 X[byte]=idx 且 idx 值集覆盖 0..93。"""
    groups = defaultdict(dict)
    for m in re.finditer(r"(\w+)\[(\d+)\]=(\d+)(?![\w.(])", code):
        ident, key, val = m.group(1), int(m.group(2)), int(m.group(3))
        groups[ident][key] = val
    for ident, tab in groups.items():
        vals = set(tab.values())
        if len(tab) >= 94 and vals == set(range(94)):
            alpha = [0] * 94
            for byte, idx in tab.items():
                alpha[idx] = byte
            return bytes(alpha)
    return None


def extract_tokens(code):
    """TK 表（v15 形态）：X[CHAR(c1..c5)]=CHAR(s) ×10。返回 {token: special}。"""
    groups = defaultdict(dict)
    for m in re.finditer(
            r"(\w+)\[(\w+)\(((?:\d+,)*\d+)\)\]=\2\((\d+)\)", code):
        ident, codes, s = m.group(1), m.group(3), int(m.group(4))
        groups[ident][codes] = s
    for ident, tab in groups.items():
        if len(tab) == 10 and sorted(tab.values()) == sorted(SPECIALS):
            tok = {}
            for codes, s in tab.items():
                t = bytes(int(x) for x in codes.split(","))
                tok[t] = s
            return tok
    return None


def unescape_carrier(enc, reserved, tokens):
    s = bytearray()
    p = 0
    while p < len(enc):
        if enc[p] == reserved:
            t = bytes(enc[p:p + 5])
            hit = tokens.get(t)
            if hit is None:
                return None
            s.append(hit)
            p += 5
        else:
            s.append(enc[p])
            p += 1
    return bytes(s)


def base94_decode(s, alphabet):
    inv = {c: i for i, c in enumerate(alphabet)}
    if len(s) % 5 != 0:
        return None
    raw = bytearray()
    for i in range(0, len(s), 5):
        v = 0
        for c in s[i:i + 5]:
            d = inv.get(c)
            if d is None:
                return None
            v = v * 94 + d
        raw += v.to_bytes(4, "little")
    ln = int.from_bytes(raw[:4], "little")
    if ln > len(raw) - 4:
        return None
    return bytes(raw[4:4 + ln])


def de_xor(seg, ks_const):
    return bytes(b ^ ((ks_const + i + 1) % 256) for i, b in enumerate(seg))


def words_to_bytes(words):
    out = bytearray()
    for w in words:
        out += int(w).to_bytes(4, "little")
    return bytes(out)


def parse_blob(b):
    """按文档化规整布局解析。返回 (consts, fully_parsed)。"""
    p = 0

    def u16():
        nonlocal p
        if p + 2 > len(b):
            raise EOFError()
        v = b[p] + b[p + 1] * 256
        p += 2
        return v

    try:
        nregs = u16()
        nparams = u16()
        if nregs > 4096 or nparams > 255:
            return None, False
        vararg = b[p]
        p += 1
        if vararg > 7:
            return None, False
        nups = u16()
        if nups > 255:
            return None, False
        p += 2 * nups
        nconst = u16()
        if nconst > 4096:
            return None, False
        consts = []
        for _ in range(nconst):
            t = b[p]
            p += 1
            if t == 0:
                consts.append(("nil", b""))
            elif t == 1:
                consts.append(("bool", bytes([b[p]])))
                p += 1
            elif t in (2, 3):
                l = u16()
                consts.append(("num" if t == 2 else "str", bytes(b[p:p + l])))
                p += l
            else:
                return None, False
        ns = u16()
        if ns > 4096:
            return None, False
        p += 2 * ns
        ncode = u16()
        if ncode > 65535 or p + ncode > len(b):
            return None, False
        p += ncode  # W
        for _ in range(4):  # 4 条 7-bit varint 流
            for _ in range(ncode):
                b1 = b[p]
                p += 1
                if b1 >= 128:
                    b2 = b[p]
                    p += 1
                    if b2 >= 128:
                        b3 = b[p]
                        p += 1
                        if b3 >= 128:
                            p += 1
        return consts, p == len(b)
    except (EOFError, IndexError):
        return None, False


def attack(path):
    """全链脱壳。返回 {blobs, consts, stages_ok, detail}；失败环节记入 detail。"""
    src = open(path, encoding="utf-8").read()
    code = strip_strings(src)
    detail = {}

    bw = extract_bw(code)
    detail["bw_words"] = len(bw)
    if len(bw) < 16:
        return {"blobs": [], "consts": [], "detail": detail,
                "fail": "no BW word table"}

    lcg = extract_lcg(code)
    detail["lcg"] = bool(lcg)
    if not lcg:
        return {"blobs": [], "consts": [], "detail": detail,
                "fail": "no LCG generator"}
    ks_slot, trips = lcg

    seed = extract_ks_seed(code, ks_slot)
    detail["ks_seed"] = seed is not None
    if seed is None:
        return {"blobs": [], "consts": [], "detail": detail,
                "fail": "no ks seed field"}

    chunks = extract_chunk_handlers(code)
    detail["chunk_handlers"] = len(chunks)
    if not chunks:
        return {"blobs": [], "consts": [], "detail": detail,
                "fail": "no chunk handlers"}

    alphabet = extract_alphabet(code)
    detail["alphabet"] = alphabet is not None
    tokens = extract_tokens(code)
    detail["tokens"] = tokens is not None
    if alphabet is None or tokens is None:
        return {"blobs": [], "consts": [], "detail": detail,
                "fail": "no AL/TK tables"}
    reserved = next(iter(tokens))[0]

    byt = words_to_bytes(bw)
    ks_state = seed
    carriers = defaultdict(bytearray)
    # 密钥流在构建期按 idx(=chunked 顺序) 消耗，必须按 idx 排序重放
    for (sw, ew, blen, idx) in sorted(chunks, key=lambda c: c[3]):
        for (m, c) in trips:
            ks_state = (m * ks_state + c) % MOD
        seg = byt[(sw - 1) * 4:ew * 4]
        seg = seg[:blen]
        carriers[(idx - 1) // 5].extend(de_xor(seg, ks_state))

    blobs, consts, stages_ok = [], [], True
    for k in sorted(carriers):
        enc = bytes(carriers[k])
        s = unescape_carrier(enc, reserved, tokens)
        if s is None:
            stages_ok = False
            detail.setdefault("unescape_fail", []).append(k)
            continue
        blob = base94_decode(s, alphabet)
        if blob is None:
            stages_ok = False
            detail.setdefault("base94_fail", []).append(k)
            continue
        blobs.append(blob)
        c, full = parse_blob(blob)
        if c is None:
            stages_ok = False
            detail.setdefault("blob_parse_fail", []).append(k)
            continue
        detail.setdefault("blob_full_parse", []).append(full)
        consts.extend(c)
    return {"blobs": blobs, "consts": consts, "stages_ok": stages_ok,
            "detail": detail, "fail": None}


if __name__ == "__main__":
    import sys
    r = attack(sys.argv[1])
    if r["fail"]:
        print("ATTACK FAILED at:", r["fail"], r["detail"])
    else:
        nstr = sum(1 for t, v in r["consts"] if t == "str")
        nnum = sum(1 for t, v in r["consts"] if t == "num")
        print(f"ATTACK OK: blobs={len(r['blobs'])} consts={len(r['consts'])}"
              f" (str={nstr} num={nnum}) stages_ok={r['stages_ok']}")
        for t, v in r["consts"]:
            if t == "str":
                print("  str:", v[:60])
            elif t == "num":
                print("  num:", v.decode("ascii", "replace"))
