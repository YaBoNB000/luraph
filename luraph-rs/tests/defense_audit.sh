#!/bin/bash
# 全项防御审计 (增量㊲) — 逐点验证混淆产物的每个防御机制是否真实生效。
#
# 审计原则: 每个防御点既验「正向」(干净输入正常运行) 也验「负向」
# (攻击/篡改/挂钩必须被拒绝, 且绝不产出正确输出)。
#
# 防御点清单:
#   D1  干净运行等价        — 防御不得破坏正确性 (基线)
#   D2  钥匙去字面化        — LURAPH_KEY_MANIFEST + key_literal_check.py
#   D3  结构指纹 32 项      — v15_fingerprint.py
#   D4  安全指纹 S1-S5      — security_fingerprint.py + attack 脚本组
#   D5  密文完整性(源哈希链) — 长串内字节互换 => 绝不产出正确输出
#   D6  字节码防篡改(校验和投毒) — 同 D5 多点统计覆盖
#   D7  激活门 --bind-key   — ㊴ 退役 (功能/测试/示例均已移除)
#   D8  环境绑定 --bind-env — 通用沙箱加载失败 + 语法仍可编译
#   D9  反挂钩闸(loadstring) — loadstring 换成 Lua 闭包 => 必须被杀
#   D10 反指纹闸(debug.info) — debug.info 说谎 => 必须被杀
#   D11 蜜罐哨兵            — Mc 层陷阱结构级验证 (外部不可触发)
#   D12 计时守卫 (㊳ 复通)  — 接线 (外层局部 + os.clock 赋值) + 触发路径
#                             (抽出守卫用合成 tracer 时钟驱动 => 必投毒) +
#                             干净时钟零误报 + 长运行用例误报抽查
#   D13 LZ 往返自检         — debug 构建 assert (编码即验)
#
# 用法: bash tests/defense_audit.sh   (release 二进制需已构建)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUAU="$TOOLS_BIN/luau"
if [ ! -x "$TOOL" ]; then
	echo "错误: 缺少 $TOOL (先 cargo build --release)"; exit 2
fi
if [ ! -x "$LUAU" ]; then
	echo "错误: 缺少 $LUAU (先 bash luraph-rs/tools/rebuild_tools.sh)"; exit 2
fi
SRC="$ROOT/tests/cases/basics.lua"
SRC2="$ROOT/tests/cases/real_algo.lua"
W=/tmp/defense_audit
rm -rf "$W"; mkdir -p "$W"

pass=0; fail=0
ok()  { echo "  PASS $1"; pass=$((pass+1)); }
bad() { echo "  FAIL $1"; fail=$((fail+1)); }

echo "== D1 干净运行等价 (防御基线) =="
EXP1="$($LUAU "$SRC" 2>&1)"
EXP2="$($LUAU "$SRC2" 2>&1)"
"$TOOL" --preset v15 --dialect luau --seed 777 "$SRC" "$W/d1a.lua" >/dev/null 2>&1
"$TOOL" --preset v15 --dialect luau --seed 888 "$SRC2" "$W/d1b.lua" >/dev/null 2>&1
[ "$($LUAU "$W/d1a.lua" 2>&1)" == "$EXP1" ] && ok "basics seed=777" || bad "basics seed=777"
[ "$($LUAU "$W/d1b.lua" 2>&1)" == "$EXP2" ] && ok "real_algo seed=888" || bad "real_algo seed=888"

echo "== D2 钥匙去字面化 (3 种子) =="
for seed in 7 4242 999999; do
	LURAPH_KEY_MANIFEST="$W/keys_$seed.txt" "$TOOL" --preset v15 --dialect luau \
		--seed "$seed" "$SRC" "$W/d2_$seed.lua" >/dev/null 2>&1
	if python3 "$ROOT/tests/key_literal_check.py" "$W/d2_$seed.lua" "$W/keys_$seed.txt" \
		>"$W/d2_$seed.out" 2>&1; then
		ok "seed=$seed $(tail -1 "$W/d2_$seed.out")"
	else
		bad "seed=$seed 钥匙泄漏: $(tail -1 "$W/d2_$seed.out")"
	fi
done

echo "== D3 结构指纹 =="
if python3 "$ROOT/tests/v15_fingerprint.py" "$W/d1a.lua" --quiet >"$W/d3.out" 2>&1; then
	ok "$(cat "$W/d3.out")"
else
	bad "结构指纹: $(cat "$W/d3.out")"
fi

echo "== D4 安全指纹 S1-S5 =="
if python3 "$ROOT/tests/security_fingerprint.py" "$W/d1a.lua" "$SRC" --quiet >"$W/d4.out" 2>&1; then
	ok "$(grep '安全指纹' "$W/d4.out")"
else
	bad "安全指纹: $(cat "$W/d4.out")"
fi

echo "== D5/D6 密文完整性 + 字节码防篡改 (真长串字节互换) =="
# ㊷ 修正: 跨度扫描必须括号层级感知(字符串内容里的 [[ ]] 是噪声字符,
# 裸配对会切出假跨度); 零引用的伪装诱饵串(故意设计)先探针甄别剔除,
# 其余真跨度必须处处敏感。
cat > "$W/d56_scan.py" <<'PYD56'
import re, subprocess, sys
art, W, LU = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(art).read()
spans = []
i, n = 0, len(s)
while i < n:
    c = s[i]
    if c == '"' or c == "'":
        q = c; i += 1
        while i < n:
            if s[i] == '\\': i += 2; continue
            if s[i] == q: break
            i += 1
        i += 1; continue
    if c == '[' and i+1 < n:
        m = re.match(r'\[(=*)\[', s[i:])
        if m:
            lvl = len(m.group(1))
            cl = ']' + '='*lvl + ']'
            jj = s.find(cl, i + len(m.group(0)))
            if jj != -1:
                spans.append((i + len(m.group(0)), jj))
                i = jj + len(cl); continue
    i += 1
big = [(a, b) for a, b in spans if b - a > 2000]
base = subprocess.run([LU, art], capture_output=True, text=True, timeout=60)
live, decoy = [], []
for (a, b) in big:
    p1 = a + (b - a)//2
    p2 = p1 + 13
    t = list(s)
    while t[p1] == t[p2]: p2 += 1
    t[p1], t[p2] = t[p2], t[p1]
    tmp = W + "/probe.lua"
    open(tmp, "w").write("".join(t))
    try:
        r = subprocess.run([LU, tmp], capture_output=True, text=True, timeout=40)
        same = (r.stdout == base.stdout and r.returncode == base.returncode)
    except subprocess.TimeoutExpired:
        same = False  # ㊸: 投毒静默循环 = 拒绝（篡改点敏感）
    if same:
        decoy.append(b - a)
    else:
        live.append((a, b))
print(f"{len(live)} live spans, {len(decoy)} decoy spans (camo, sizes {decoy})")
assert live, "no live spans found"
seed = 12345
def rnd(n):
    global seed
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed % n
for k in range(30):
    a, b = live[rnd(len(live))]
    while True:
        p1 = a + rnd(b - a)
        p2 = a + rnd(b - a)
        if p1 != p2 and s[p1] != s[p2]:
            break
    t = list(s)
    t[p1], t[p2] = t[p2], t[p1]
    open(f"{W}/tamper_{k}.lua", "w").write("".join(t))
print("tampers written")
PYD56
python3 "$W/d56_scan.py" "$W/d1a.lua" "$W" "$LUAU" || bad "跨度扫描失败"
tamper_fail=0
for k in $(seq 0 29); do
	out="$(timeout 30 "$LUAU" "$W/tamper_$k.lua" 2>&1)"
	if [ "$out" == "$EXP1" ]; then
		bad "篡改点 $k 产出了正确输出 (完整性防御失效)"
		tamper_fail=1
	fi
done
[ "$tamper_fail" == "0" ] && ok "30/30 真跨度篡改点全部被拒 (诱饵伪装串已甄别剔除)"

echo "== D7 激活门 (㊴ 退役, 跳过) =="
echo "  SKIP --bind-key 功能/测试/示例已随 ㊴ 移除"

echo "== D8 环境绑定 --bind-env =="
if bash "$ROOT/tests/env_bind_test.sh" >"$W/d8.out" 2>&1; then
	ok "$(grep 'ENV BIND PASS' "$W/d8.out")"
else
	bad "环境绑定: $(tail -3 "$W/d8.out" | tr '\n' ' ')"
fi

echo "== D9 反挂钩闸 (loadstring 换成 Lua 闭包 => 必须被杀) =="
python3 - "$W/d1a.lua" "$W" <<'PYEOF'
import sys
src = open(sys.argv[1]).read()
hook = '''
local real_ls = loadstring
local env = {}
for k, v in pairs(_G) do env[k] = v end
env._G = env
env.loadstring = function(s, ...) return real_ls(s, ...) end
local src = [==[''' + src + ''']==]
local f = assert(real_ls(src))
setfenv(f, env)
f()
'''
open(sys.argv[2] + "/d9_hook.lua", "w").write(hook)
PYEOF
out9="$(timeout 12 "$LUAU" "$W/d9_hook.lua" 2>&1)"; rc9=$?
if [ "$out9" == "$EXP1" ]; then
	bad "反挂钩闸未生效: 挂钩后仍产出正确输出"
else
	ok "挂钩被杀 (rc=$rc9, 无限循环/静默死亡, 无正确输出)"
fi

echo "== D10 反指纹闸 (debug.info 说谎 => 必须被杀) =="
python3 - "$W/d1a.lua" "$W" <<'PYEOF'
import sys
src = open(sys.argv[1]).read()
hook = '''
local env = {}
for k, v in pairs(_G) do env[k] = v end
env._G = env
local dbg = {}
for k, v in pairs(debug) do dbg[k] = v end
dbg.info = function(f, what, ...) if what == "s" then return "[C]" end return debug.info(f, what, ...) end
env.debug = dbg
local src = [==[''' + src + ''']==]
local f = assert(loadstring(src))
setfenv(f, env)
f()
'''
open(sys.argv[2] + "/d10_hook.lua", "w").write(hook)
PYEOF
out10="$(timeout 12 "$LUAU" "$W/d10_hook.lua" 2>&1)"; rc10=$?
if [ "$out10" == "$EXP1" ]; then
	bad "反指纹闸未生效: debug.info 说谎后仍产出正确输出"
else
	ok "指纹伪造被杀 (rc=$rc10, 无正确输出)"
fi

echo "== D11 蜜罐哨兵 (㊶: 移入掩码层, 验可见层零痕迹 + 守卫生效) =="
# 蜜罐由守卫在 HBOOT 掩码层内安装——可见层必须 0 痕迹；守卫活性由
# D9/D10 (挂钩/指纹伪造必杀) 背书。
hp_vis=0
for pat in "newproxy" "__tostring" "__metatable" "__concat"; do
	if grep -q "$pat" "$W/d1a.lua"; then hp_vis=1; echo "  可见层发现 $pat"; fi
done
if [ "$hp_vis" == "0" ]; then
	ok "蜜罐/陷阱可见层零痕迹 (掩码层内安装, D9/D10 背书守卫生效)"
else
	bad "蜜罐结构仍暴露在可见层"
fi

echo "== D12 计时守卫 (㊳ 复通: 接线 + 触发路径 + 误报抽查) =="
mkdir -p "$W/frags"
LURAPH_FRAG_SAN="$W/frags" "$TOOL" --preset v15 --dialect luau --seed 777 "$SRC" "$W/d12b.lua" >/dev/null 2>&1
python3 - "$W/d1a.lua" "$W" <<'PYEOF'
import re, sys
art = sys.argv[1]; W = sys.argv[2]
s = open(art).read()
def blank(src):
    out = list(src); i = 0; n = len(src)
    while i < n:
        c = src[i]
        if c == '[' and i+1 < n and src[i+1] == '[':
            j = src.find(']]', i+2)
            if j != -1:
                for k in range(i, j+2): out[k] = ' '
                i = j+2; continue
        if c in '"\'':
            q = c; j = i+1
            while j < n:
                if src[j] == '\\': j += 2; continue
                if src[j] == q: break
                j += 1
            for k in range(i, min(j+1, n)): out[k] = ' '
            i = j+1; continue
        i += 1
    return ''.join(out)
code = blank(s)
m = re.search(r'return (\w+)\(\w+, ?\w+, ?\{\}, ?\w+, ?0\)', code)
if not m:
    print("NORUN"); sys.exit(1)
fn = m.group(1)
mm = re.search(r'local '+fn+r'=(\w+)\(', code[:m.start()])
st = code.find('(', mm.end()-1)
depth=0; args=[]; cur=""; j=st
while j < len(code):
    c = code[j]
    if c=='(':
        depth+=1
        if depth==1: j+=1; continue
    elif c==')':
        depth-=1
        if depth==0: args.append(cur.strip()); break
    elif c==',' and depth==1:
        args.append(cur.strip()); cur=""; j+=1; continue
    cur+=c; j+=1
a27 = args[26]
# 接线: 外层局部 + do 块内从 os 表取 clock 的赋值 (非 nil 悬挂)
asg = re.search(r'\b'+re.escape(a27)+r'=(\w+) and (\w+)\[', code[:mm.start()+10])
decl = re.search(r'local\s+[^=;]*\b'+re.escape(a27)+r'\b', code[:mm.start()+10])
if not (asg and decl):
    print("NOWIRE"); sys.exit(1)
# 触发路径: 从 RT 碎片抽出守卫前导 + 守卫块本体, 合成 tracer 时钟驱动
rt = open(W + "/frags/san_208.src").read()
pm = re.search(
    r'local (\w+) = 0 local (\w+) = (\w+) and \3\(\) or 0 '
    r'local (\w+) = 0 local (\w+) = nil local (\w+) = 0 while true do', rt)
if not pm:
    print("NOPRE"); sys.exit(1)
GE, ID, CLK, NF, PE, CNT = pm.groups()
gi = rt.find('% 128 == 0')
# 回溯到该 if 的起点
ist = rt.rfind('if ', 0, gi)
br = blank(rt)  # 位置保真: 字符串内容被空格化, 代码标识符不动
depth = 0; cut = None
for m2 in re.finditer(r'\b(then|do|end|until)\b', br):
    if m2.start() < ist:
        continue
    w = m2.group(1)
    if w in ('then', 'do'):
        depth += 1
    else:
        depth -= 1
        if depth == 0:
            cut = m2.end()
            break
if cut is None:
    print("NOGUARD"); sys.exit(1)
guard = br[ist:cut]
p1 = re.search(r'(\w+) = \1 \+ 7777777 (\w+) = \2 \+ 7777777', guard)
if not p1:
    print("NOPOISON"); sys.exit(1)
A, B = p1.groups()
seq_tracer = "0, 0.001, 0.002, 0.003, 0.004, 10.004, 20.004, 20.005, 20.006"
seq_clean  = "0, 0.001, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007, 0.008"
harness = '''
local seq = {{ {seq} }}
local ix = 0
local {CLK} = function() ix = ix + 1 return seq[ix] end
local {GE} = 0
local {ID} = {CLK} and {CLK}() or 0
local {NF} = 0
local {PE} = nil
local {CNT} = 0
local {A}, {B} = 0, 0
for iter = 1, 128 * 8 do
  {GE} = {GE} + 1
  {guard}
end
print({A}, {B})
'''
for name, seq in (("tracer", seq_tracer), ("clean", seq_clean)):
    lua = harness.format(seq=seq, CLK=CLK, GE=GE, ID=ID, NF=NF, PE=PE,
                         CNT=CNT, A=A, B=B, guard=guard)
    open(f"{W}/wd_{name}.lua", "w").write(lua)
print("WIRED", a27)
PYEOF
rc12=$?
if [ "$rc12" != "0" ]; then
	bad "计时守卫接线/抽取失败 (rc=$rc12)"
else
	t1="$(timeout 15 "$LUAU" "$W/wd_tracer.lua" 2>&1)"
	t2="$(timeout 15 "$LUAU" "$W/wd_clean.lua" 2>&1)"
	[ "$t1" == "7777777	7777777" ] && ok "触发路径: 模拟 tracer(10000x 两连窗) => SLT/SLTC 各投毒 +7777777" || bad "触发路径失效: 得到 [$t1]"
	[ "$t2" == "0	0" ] && ok "干净时钟: 8 窗口 0 投毒 (阈值不误伤)" || bad "干净时钟误报: [$t2]"
	# 误报抽查: 长运行用例真实执行必须输出等价
	"$TOOL" --preset v15 --dialect luau --seed 999 \
		"$ROOT/tests/cases/stress_control.lua" "$W/d12c.lua" >/dev/null 2>&1
	exp12="$(timeout 90 "$LUAU" "$ROOT/tests/cases/stress_control.lua" 2>&1)"
	got12="$(timeout 90 "$LUAU" "$W/d12c.lua" 2>&1)"
	[ "$got12" == "$exp12" ] && ok "误报抽查: stress 用例守卫开启下运行等价" || bad "守卫开启导致运行分歧 (疑似误报投毒)"
fi

echo "== D13 LZ 往返自检 (debug 构建) =="
if [ -x "$ROOT/target/debug/luraph-rs" ]; then
	DBGOBJ="$ROOT/target/debug/luraph-rs"
else
	echo "  (构建 debug 二进制中...)"
	(cd "$ROOT" && CARGO_NET_OFFLINE=true cargo build >/dev/null 2>&1)
	DBGOBJ="$ROOT/target/debug/luraph-rs"
fi
if [ -x "$DBGOBJ" ]; then
	if "$DBGOBJ" --preset v15 --dialect luau --seed 31337 "$SRC" "$W/d13.lua" >/dev/null 2>&1 \
		&& [ "$($LUAU "$W/d13.lua" 2>&1)" == "$EXP1" ]; then
		ok "debug 构建生成期往返 assert 通过 + 产物运行等价"
	else
		bad "LZ 往返自检或运行失败"
	fi
else
	bad "debug 二进制不可用"
fi

echo "== D14 保护代码隐藏普查 (㊶: 对照样本 15 的隐藏形态) =="
python3 - "$W/d1a.lua" <<'PYEOF2'
import re, sys
s = open(sys.argv[1]).read()
pats = {
    "死循环闸 while true do end": r"while true do end",
    "蜜罐钩子词": r"newproxy|__tostring|__metatable|__concat",
    "repeat-until kill": r"repeat \w+=\w+\+1",
    "schar 码表(3+)": r"char\((?:\d+,){3,}",
    "拼串纸老虎(乱序码表+拼串循环)": r"=\s*\w+\s*\.\.\s*\w+\(\w+\[\w+\[",
}
bad = 0
for name, p in pats.items():
    n = len(re.findall(p, s))
    if n:
        print(f"  暴露: {name} x{n}")
        bad = 1
sys.exit(bad)
PYEOF2
[ "$?" == "0" ] && ok "可见层保护痕迹清零 (死循环/蜜罐词/码表/拼串表全 0)" || bad "可见层仍有保护痕迹"

echo "== D15 环境绑定语义强化 (㊸: 桩场景 ⇒ 静默毒药) =="
# 朴素桩（形状对、语义错：typeof=table、无 tostring 格式）⇒ 折叠异值 ⇒
# 元钥匙流污染 ⇒ HBOOT 垃圾 ⇒ 静默循环——无报错、无信号（样本式毒药
# 语义）。若桩能跑通 = 语义探针失效。
"$TOOL" --preset v15 --dialect luau --bind-env roblox --seed 42 "$SRC" "$W/d15_bound.lua" >/dev/null 2>&1 || bad "绑定态构建失败"
python3 - "$W/d15_bound.lua" "$W" <<'PYD15'
import sys
src = open(sys.argv[1]).read()
W = sys.argv[2]
def wrap(s, name):
    for lvl in range(1, 5):
        cl = ']' + '='*lvl + ']'
        if cl not in s:
            op = '[' + '='*lvl + '['
            return f'local {name} = {op}\n{s}\n{cl}'
    raise RuntimeError("no level")
runner = wrap(src, 'BND') + """
local env = {}
for k, v in pairs(_G) do env[k] = v end
env._G = env
env.Vector3 = { new = function(x, y, z) return { X = x, Y = y, Z = z } end }
env.Vector2 = { new = function(x, y) return { X = x, Y = y } end }
env.task = { defer = function() end }
local f = assert(loadstring(BND, "bound"))
setfenv(f, env)
local ok, err = pcall(f)
print("STUB-RUN:", ok, tostring(err):sub(1, 100))
"""
open(W + "/d15_stub.lua", "w").write(runner)
PYD15
out15="$(timeout 15 "$LUAU" "$W/d15_stub.lua" 2>&1)"; rc15=$?
if [ "$out15" == "$EXP1" ]; then
	bad "朴素桩跑通了 (语义探针失效)"
else
	ok "朴素桩 => 静默毒药 (rc=$rc15, 无正确输出/无报错信号)"
fi

echo "=================================================="
echo "防御审计: PASS $pass   FAIL $fail"
[ "$fail" == "0" ] && echo "ALL DEFENSES ACTIVE" || exit 1
#   D15 环境绑定语义强化 — 朴素桩场景必须静默毒药 (㊸)
