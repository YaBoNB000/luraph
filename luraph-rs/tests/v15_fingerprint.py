#!/usr/bin/env python3
"""v15 结构指纹检查器（P0 脚手架，路线 A 已拍板 2026-08-25）。

对照 `samples/luraph15.txt` 的黄金数，统计一份混淆输出的 32 条结构指纹
（F1–F32，定义见 `docs/v15-structural-parity-plan.md` §0.1/§0.2）。

用法：
    python3 tests/v15_fingerprint.py <file.lua> [--quiet]
退出码：0 = 32 条全部通过；1 = 有失败。

解析口径（二轮详读实测踩过的坑，全部已处理）：
  * 统计前先剥掉长字符串（RC blob）区间；
  * 块计数必须排除 Luau **if 表达式**（`=if ...then...else...` 无 end，
    不排除则 end 平衡差 17、模块表字段识别全错）；
  * 模块表字段按**函数深度 0** 识别（`function...end` 不改花括号深度，
    纯括号深度会把 handler 体内的赋值误判为表字段）。

黄金数 = 样本实测（2026-08-25 二轮复测）；阈值 = 验收下限（不必等于样本，
进入同数量级即可——每构建随机面会改变具体数值）。
"""
import re
import sys

PUSHKW = frozenset(("function", "if", "do", "repeat"))
POPKW = frozenset(("end", "until"))
PRIM_PREFIXES = (
    "buffer.", "bit32.", "string.", "table.", "coroutine.", "Vector",
    "vector.", "typeof", "setfenv", "getfenv", "pcall", "xpcall",
    "error", "next", "select", "unpack", "tonumber", "tostring",
    "rawget", "rawset", "assert", "type", "setmetatable", "getmetatable",
)


def tokenize(src):
    """剥字符串/注释/长字符串。返回:
    code      —— 同长度文本，字符串内容与注释被空格替换（位置保真）
    strings   —— [(start, text)] 双引号字符串（长字符串之外）
    longstrs  —— [(start, end)] 长字符串区间
    """
    n = len(src)
    code = list(src)
    strings = []
    longstrs = []
    i = 0
    in_str = None      # None | '"' | "'" | ('long', level)
    esc = False
    while i < n:
        ch = src[i]
        if in_str is not None:
            if isinstance(in_str, tuple):  # long string
                lvl = in_str[1]
                if src.startswith("]" + "=" * lvl + "]", i):
                    for k in range(i, i + lvl + 2):
                        code[k] = " "
                    longstrs.append((start, i + lvl + 2))
                    in_str = None
                    i += lvl + 2
                    continue
                code[i] = " "
                i += 1
                continue
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif (in_str == '"' and ch == '"') or (in_str == "'" and ch == "'"):
                strings.append((start, src[start:i + 1]))
                in_str = None
            else:
                code[i] = " "
            i += 1
            continue
        if ch == '"':
            in_str = '"'
            start = i
        elif ch == "'":
            in_str = "'"
            start = i
        elif ch == "[":
            m = re.match(r"\[(=*)\[", src[i:i + 12])
            if m:
                in_str = ("long", len(m.group(1)))
                start = i
                i += len(m.group(0))
                for k in range(start, i):
                    code[k] = " "
                continue
        elif ch == "-" and src.startswith("--", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                code[k] = " "
            i = j
            continue
        i += 1
    return "".join(code), strings, longstrs


def block_scan(code, cb):
    """关键字级块扫描（if 表达式修正）。cb(kind, word, i, blk)。"""
    n = len(code)
    i = 0
    blk = 0
    while i < n:
        ch = code[i]
        if ch.isalpha() or ch == "_":
            j = i
            while j < n and (code[j].isalnum() or code[j] == "_"):
                j += 1
            w = code[i:j]
            if w in PUSHKW:
                if w == "if" and i > 0 and code[i - 1] == "=":
                    cb("kw", w, i, blk)  # if 表达式：不计块
                else:
                    blk += 1
                    cb("kw", w, i, blk)
            elif w in POPKW:
                cb("kw", w, i, blk)
                blk -= 1
            else:
                cb("kw", w, i, blk)
            i = j
            continue
        cb("ch", ch, i, blk)
        i += 1


def module_fields(code):
    """模块表字段识别。返回 (numslots, named_fns, named_consts, tstart, tend)。"""
    m = re.search(r"setmetatable\(\{", code)
    if not m:
        return {}, [], {}, -1, -1
    tstart = m.end() - 1
    # 找匹配的右括号（字符串已剥掉，直接数括号）
    d = 0
    tend = -1
    for i in range(tstart, len(code)):
        if code[i] == "{":
            d += 1
        elif code[i] == "}":
            d -= 1
            if d == 0:
                tend = i
                break
    if tend < 0:
        return {}, [], {}, -1, -1
    # 函数范围（模块级 =function 起，blk 回到 0 止）
    numslots = {}
    named_fns = []
    named_consts = {}
    blk = 0
    i = tstart
    n = len(code)

    def key_before(eqpos):
        j = eqpos - 1
        while j >= 0 and code[j] == " ":
            j -= 1
        if j >= 0 and code[j] == "]":
            k2 = code.rfind("[", 0, j)
            if k2 >= 0 and re.fullmatch(r"\d+", code[k2 + 1:j]):
                return int(code[k2 + 1:j])
            return None
        m2 = re.search(r"([A-Za-z_][A-Za-z_0-9]*)$", code[:j + 1])
        if not m2:
            return None
        st = m2.start()
        before = code[st - 1] if st > 0 else ""
        if before in "{,":
            return m2.group(1)
        return None

    fn_stack = []
    i = tstart
    blk = 0
    pending_key = None
    while i < tend:
        ch = code[i]
        if ch.isalpha() or ch == "_":
            j = i
            while j < n and (code[j].isalnum() or code[j] == "_"):
                j += 1
            w = code[i:j]
            if w in PUSHKW:
                if not (w == "if" and i > 0 and code[i - 1] == "="):
                    blk += 1
                if w == "function" and blk == 1 and i > 0 and code[i - 1] == "=":
                    fn_stack.append(key_before(i))
            elif w in POPKW:
                blk -= 1
                if blk == 0 and fn_stack:
                    fn_stack.pop()
            i = j
            continue
        if ch == "=" and blk == 0 and i + 1 < tend and code[i + 1] != "=" \
                and code[i - 1] not in "<>~":
            key = key_before(i)
            val = code[i + 1:i + 80].split(",")[0].strip()
            if isinstance(key, int):
                numslots[key] = val
            elif isinstance(key, str):
                if val.startswith("function"):
                    named_fns.append(key)
                else:
                    named_consts[key] = val
        i += 1
    return numslots, named_fns, named_consts, tstart, tend


def function_ranges(code):
    """模块级函数 (name, start, end) 列表。"""
    m = re.search(r"setmetatable\(\{", code)
    if not m:
        return []
    tstart = m.end() - 1
    ranges = []
    starts = []
    n = len(code)
    blk = 0
    i = tstart
    while i < n:
        ch = code[i]
        if ch.isalpha() or ch == "_":
            j = i
            while j < n and (code[j].isalnum() or code[j] == "_"):
                j += 1
            w = code[i:j]
            if w in PUSHKW:
                if not (w == "if" and i > 0 and code[i - 1] == "="):
                    blk += 1
                if w == "function" and blk == 1 and i > 0 and code[i - 1] == "=":
                    j2 = i - 2
                    while j2 >= 0 and code[j2] == " ":
                        j2 -= 1
                    key = None
                    if code[j2] == "]":
                        k2 = code.rfind("[", 0, j2)
                        if k2 >= 0:
                            key = code[k2 + 1:j2]
                    else:
                        m2 = re.search(r"([A-Za-z_][A-Za-z_0-9]*)$", code[:j2 + 1])
                        if m2:
                            key = m2.group(1)
                    starts.append((key, i))
            elif w in POPKW:
                blk -= 1
                if blk == 0 and starts:
                    key, s = starts.pop()
                    ranges.append((key, s, i))
            i = j
            continue
        i += 1
    return ranges


def check(name, desc, measured, golden, ok):
    return {"id": name, "desc": desc, "measured": measured,
            "golden": golden, "ok": bool(ok)}


def analyze(path):
    src = open(path, encoding="utf-8", errors="replace").read()
    code, strings, longstrs = tokenize(src)
    lines = src.count("\n") + (0 if src.endswith("\n") else 1)
    phys = src.split("\n")
    numslots, named_fns, named_consts, tstart, tend = module_fields(code)
    franges = function_ranges(code)
    F = []

    # ---- F1 文件形态：3 行（注释 + 空行 + return），或 2 行（无注释头）
    if lines == 3:
        ok1 = phys[0].startswith("--") and phys[1].strip() == "" \
            and phys[2].startswith("return setmetatable")
    elif lines == 2:
        ok1 = phys[1].startswith("return setmetatable")
    else:
        ok1 = False
    F.append(check("F1", "3 行:注释+空行+return(或 2 行无头)", lines, 3, ok1))

    # ---- F2 入口 return setmetatable({...},{}):X()(...);
    tail = src.rstrip()
    ok2 = bool(re.search(r"return setmetatable\(", code)) and \
        bool(re.search(r":\w+\(\)\(\.\.\.\);\s*$", tail))
    F.append(check("F2", "setmetatable 入口 + 结尾分号", 1 if ok2 else 0, 1, ok2))

    # ---- F3 模块表字段数
    total_fields = len(numslots) + len(named_fns) + len(named_consts)
    F.append(check("F3", "模块表字段数", total_fields, 227, total_fields >= 100))

    # ---- F4 =function( 总数 + 具名函数数
    eqfn = len(re.findall(r"=function\(", code))
    F.append(check("F4", "=function( / 具名函数", (eqfn, len(named_fns)),
                   (146, 141), eqfn >= 100 and len(named_fns) >= 100))

    # ---- F5 状态返回 / 独立状态 ID / 方法调用
    rets = re.findall(r"return (\d+),", code)
    nret, nids = len(rets), len(set(rets))
    mcalls = len(re.findall(r"\b\w+:\w+\(", code))
    F.append(check("F5", "(return N, / 独立 ID / 方法调用)",
                   (nret, nids, mcalls), (351, 136, 704),
                   nret >= 100 and nids >= 30 and mcalls >= 100))

    # ---- F6 宽参数(≥7)函数数
    wide = 0
    for _m in re.finditer(r"=function\(([^)]*)\)", code):
        plist = [p for p in _m.group(1).split(",") if p.strip()]
        if len(plist) >= 7:
            wide += 1
    F.append(check("F6", "参数≥7 的函数", wide, 124, wide >= 50))

    # ---- F7 continue 总数 + 单函数集中度
    cont = re.findall(r"\bcontinue\b", code)
    per_fn = {}
    for _m in re.finditer(r"\bcontinue\b", code):
        for key, s, e in franges:
            if s < _m.start() < e:
                per_fn[key] = per_fn.get(key, 0) + 1
                break
    mx = max(per_fn.values()) if per_fn else 0
    F.append(check("F7", "continue 数 / 最大单函数", (len(cont), mx),
                   (45, 45), len(cont) >= 20 and mx >= 20))

    # ---- F8 大长串 + pC 形转义表（单字符键 → 5 字符值）
    maxlong = max((e - s for s, e in longstrs), default=0)
    pc = len(re.findall(r'\[\s*"(?:\\.|[^"\\])"\s*\]\s*=\s*"(?:\\.|[^"\\]){5}"', code))
    F.append(check("F8", "最长长串字节 / pC 形条目", (maxlong, pc),
                   (74572, 10), maxlong >= 10000 and pc >= 8))

    # ---- F9 数字槽 + 原语槽
    prim_slots = sum(1 for v in numslots.values()
                     if v.startswith(PRIM_PREFIXES))
    F.append(check("F9", "数字槽 / 其中原语槽", (len(numslots), prim_slots),
                   (73, 68), len(numslots) >= 40 and prim_slots >= 10))

    # ---- F10 blob 外字符串字面量数
    nstr = len(strings)
    F.append(check("F10", "短字符串字面量数", nstr, 28, 10 <= nstr <= 60))

    # ---- F11 fetch 循环 local X=T[Y];if X
    fetch = len(re.findall(r"local (\w{1,2})=(\w{1,2})\[(\w{1,2})\];if \1", code))
    F.append(check("F11", "fetch 循环数", fetch, 19, fetch >= 4))

    # ---- F12 pcall 包帧 + 包装闭包
    pcallf = len(re.findall(r"\w\(function\(", code))
    wrap = len(re.findall(r"=\d+,function\(\.\.\.\)", code))
    F.append(check("F12", "pcall(function / N,function(...) 包装",
                   (pcallf, wrap), (1, 2), pcallf >= 1 and wrap >= 1))

    # ---- F13 -128 偏置 + 128 进制重建
    mb = code.count("-128")
    base = code.count("*128") + code.count("*16384") + code.count("*2097152")
    F.append(check("F13", "-128 偏置 / 128 进制乘加", (mb, base),
                   (264, 136), mb >= 50 and base >= 20))

    # ---- F14 SoA 常量自写回
    sow = re.findall(r";(\w{1,2})\[(\w+)\]=(\d+);", code)
    F.append(check("F14", "SoA 常量写回数", len(sow), 18, len(sow) >= 3))

    # ---- F15 LCG 形态 + 嵌套槽状态写
    lcg = code.count("%268435456")
    nest = len(re.findall(r"\w\[\d+\]\[\d+\]\[", code))
    F.append(check("F15", "%268435456 / 嵌套槽写入", (lcg, nest),
                   (34, 68), lcg >= 2 and nest >= 10))

    # ---- F16 %256 位置密钥 + bxor
    m256 = code.count("%256")
    bxor = code.count("bit32.bxor")
    F.append(check("F16", "%256 / bit32.bxor", (m256, bxor),
                   (109, 1), m256 >= 5 and bxor >= 1))

    # ---- F17 行号重写正则
    lnre = 1 if ":(%d+)" in src else 0
    F.append(check("F17", "行号重写正则存在", lnre, 1, lnre >= 1))

    # ---- F18 无 os.clock / 无 loadstring
    osc = len(re.findall(r"\bos\.clock\b", code))
    lds = len(re.findall(r"\bloadstring\b", code))
    F.append(check("F18", "os.clock=0 且 loadstring=0", (osc, lds),
                   (0, 0), osc == 0 and lds == 0))

    # ---- F19 Roblox 绑定
    buf = len(re.findall(r"\bbuffer\.", code))
    tof = len(re.findall(r"\btypeof\b", code))
    vec = len(re.findall(r"\bvector\.|\bVector[23]\b", code))
    F.append(check("F19", "buffer./typeof/vector", (buf, tof, vec),
                   (17, 1, 3), buf >= 10 and tof >= 1 and vec >= 1))

    # ---- F20 while 数量
    whl = len(re.findall(r"\bwhile\b", code))
    F.append(check("F20", "while 数", whl, 14, whl >= 8))

    # ---- F21 命名方案：具名函数 1–2 字符占比
    short = sum(1 for k in named_fns if len(k) <= 2)
    pct = 100 * short // len(named_fns) if named_fns else 0
    F.append(check("F21", "具名函数≤2字符% / 具名数", (pct, len(named_fns)),
                   (100, 141), pct >= 90 and len(named_fns) >= 100))

    # ---- F22 if 表达式
    ife = len(re.findall(r"=if ", code))
    F.append(check("F22", "=if 表达式数", ife, 17, ife >= 4))

    # ---- F23 融合条件 and N or M
    fus = len(re.findall(r"\band \d+ or \d+", code))
    F.append(check("F23", "and N or M 数", fus, 166, fus >= 50))

    # ---- F24 复合赋值
    cmp_ = len(re.findall(r"[+\-*/^]=[^=]", code))
    F.append(check("F24", "复合赋值数", cmp_, 41, cmp_ >= 20))

    # ---- F25 参数遮蔽（同名参数重复）
    shadow = 0
    for _m in re.finditer(r"=function\(([^)]*)\)", code):
        plist = [p.strip() for p in _m.group(1).split(",") if p.strip()]
        if len(plist) != len(set(plist)):
            shadow += 1
    F.append(check("F25", "同名参数函数数", shadow, 10, shadow >= 1))

    # ---- F26 模块表运行期自变异（槽写回 + 命名字段写回）
    slotw = len(re.findall(r"\w\[\d+\]=\w", code)) - len(numslots)
    named_w = len(re.findall(r"\b\w\.[A-Za-z_]\w*=[^=]", code))
    F.append(check("F26", "槽自变异 / 命名字段写", (max(slotw, 0), named_w),
                   (17, 1), slotw >= 1 and named_w >= 1))

    # ---- F27 自修改命中的数组多样性
    arrays = set(a for a, _k, _v in sow)
    F.append(check("F27", "自修改数组种类", len(arrays), 9, len(arrays) >= 2))

    # ---- F28 诱饵：含 268435456 的表内函数槽且静态零外部调用
    decoy = 0
    for key, s, e in franges:
        if key and re.fullmatch(r"\d+", str(key)) and "268435456" in code[s:e]:
            ext = len(re.findall(r"\w\[%s\]" % key, code[:s] + code[e:]))
            if ext == 0:
                decoy += 1
    F.append(check("F28", "诱饵 LCG 槽(零静态调用)", decoy, 2, decoy >= 1))

    # ---- F29 大常数(≥1e8)字面量
    big = len(re.findall(r"\b\d{9,}\b", code))
    F.append(check("F29", "≥1e8 常数字面量", big, 516, big >= 20))

    # ---- F30 upvalue cell 布局 [4]= / [7]=
    c4 = len(re.findall(r"\[4\]=", code))
    c7 = len(re.findall(r"\[7\]=", code))
    F.append(check("F30", "[4]= / [7]= 布局", (c4, c7), (24, 23),
                   c4 >= 3 and c7 >= 3))

    # ---- F31 元组槽 makefn：x[y[4]](
    mkfn = len(re.findall(r"\w\[\w\[4\]\]\(", code))
    F.append(check("F31", "元组槽 makefn", mkfn, 5, mkfn >= 1))

    # ---- F32 初始化器 return true,N,nil
    init = len(re.findall(r"return true,\d+,nil", code))
    F.append(check("F32", "初始化器数", init, 4, init >= 2))

    return F


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    quiet = "--quiet" in sys.argv
    if not args:
        print(__doc__)
        sys.exit(2)
    F = analyze(args[0])
    npass = sum(1 for f in F if f["ok"])
    if not quiet:
        print("v15 结构指纹：%s" % args[0])
        print("%-4s %-34s %-18s %-14s %s" %
              ("ID", "指纹", "实测", "黄金数", "结果"))
        for f in F:
            print("%-4s %-34s %-18s %-14s %s" %
                  (f["id"], f["desc"], str(f["measured"]),
                   str(f["golden"]), "PASS" if f["ok"] else "FAIL"))
    print("通过 %d/32" % npass)
    sys.exit(0 if npass == 32 else 1)


if __name__ == "__main__":
    main()
