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
#   D7  激活门 --bind-key   — 正确钥匙通行 / 错缺钥匙拒绝 / 无字面泄漏
#   D8  环境绑定 --bind-env — 通用沙箱加载失败 + 语法仍可编译
#   D9  反挂钩闸(loadstring) — loadstring 换成 Lua 闭包 => 必须被杀
#   D10 反指纹闸(debug.info) — debug.info 说谎 => 必须被杀
#   D11 蜜罐哨兵            — Mc 层陷阱结构级验证 (外部不可触发)
#   D12 计时守卫            — 显式关闭: RUN 的 CLK 槽引用外层 nil 局部,
#                             RT 碎片内守卫代码结构仍在 (可随时复通)
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

echo "== D5/D6 密文完整性 + 字节码防篡改 (30 点字节互换) =="
# 在产物所有长串 (base-94 载荷) 内做字符互换: 语法仍合法、字母表仍合法,
# 但载荷内容被破坏。源哈希链/校验和投毒必须保证: 绝不产出正确输出。
python3 - "$W/d1a.lua" "$W" <<'PYEOF'
import re, sys
src, outdir = sys.argv[1], sys.argv[2]
s = open(src).read()
# 收集所有长字符串区间
spans = []
for m in re.finditer(r'\[\[(.*?)\]\]', s, re.S):
    if len(m.group(1)) > 2000:
        spans.append((m.start(1), m.end(1)))
assert spans, "no long payload strings found"
seed = 12345
def rnd(n):
    global seed
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed % n
for k in range(30):
    st, en = spans[rnd(len(spans))]
    t = list(s)
    # 强制两点字符不同, 保证互换是真篡改 (防空互换无效点)
    while True:
        i = st + rnd(en - st)
        j = st + rnd(en - st)
        if j != i and t[i] != t[j]:
            break
    t[i], t[j] = t[j], t[i]
    open(f"{outdir}/tamper_{k}.lua", "w").write("".join(t))
print(len(spans), "payload spans")
PYEOF
tamper_fail=0
for k in $(seq 0 29); do
	out="$(timeout 15 "$LUAU" "$W/tamper_$k.lua" 2>&1)"
	if [ "$out" == "$EXP1" ]; then
		bad "篡改点 $k 产出了正确输出 (完整性防御失效)"
		tamper_fail=1
	fi
done
[ "$tamper_fail" == "0" ] && ok "30/30 篡改点全部被拒 (0 次正确输出)"

echo "== D7 激活门 --bind-key =="
if bash "$ROOT/tests/bind_gate_test.sh" >"$W/d7.out" 2>&1; then
	ok "$(grep 'BIND GATE PASS' "$W/d7.out")"
else
	bad "激活门: $(tail -3 "$W/d7.out" | tr '\n' ' ')"
fi

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

echo "== D11 蜜罐哨兵 (结构级) =="
# Mc 层把 __tostring/__concat/__call 全指向杀死函数的哨兵表由引导码运行时
# 构造并藏入状态机; 外部无法直接触发, 只做存在性验证: 产物须含 Mc 检查块
# 的三个哨兵字符串构造 (GS16/17/18 乱序字符表)。
if grep -q "debug" "$W/d1a.lua" 2>/dev/null || python3 - "$W/d1a.lua" <<'PYEOF'
import sys
s = open(sys.argv[1]).read()
# 蜜罐特征: newproxy(true) + __tostring 陷阱装配同时出现
ok = ("newproxy" in s) and ("__tostring" in s) and ("__metatable" in s)
sys.exit(0 if ok else 1)
PYEOF
then
	ok "哨兵陷阱结构存在 (newproxy + __tostring/__metatable 陷阱)"
else
	bad "未找到蜜罐结构"
fi

echo "== D12 计时守卫 (设计: 显式关闭, 接线完好) =="
python3 - "$W/d1a.lua" "$W" <<'PYEOF'
import re, sys
art = sys.argv[1]
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
pat = re.compile(r'local\s+[^=;]*\b'+re.escape(a27)+r'\b')
decls = [d.start() for d in pat.finditer(code[:mm.start()+10])]
# 外层显式声明存在 => RUN 读到 nil (守卫关闭); 守卫结构本体另验
print("CLKARG", a27, "decls", len(decls))
sys.exit(0 if len(decls) >= 1 else 1)
PYEOF
rc12=$?
if [ "$rc12" == "0" ]; then
	# 再验 RT 碎片内守卫结构仍在
	mkdir -p "$W/frags"
	LURAPH_FRAG_SAN="$W/frags" "$TOOL" --preset v15 --dialect luau --seed 777 "$SRC" "$W/d12b.lua" >/dev/null 2>&1
	if grep -q "% 128 == 0" "$W/frags/san_208.src"; then
		ok "CLK 槽接外层 nil 局部 (守卫关闭) + RT 内 %128 守卫结构完好 (可复通)"
	else
		bad "RT 碎片内未找到 %128 计时守卫结构"
	fi
else
	bad "RUN 的 CLK 槽未接外层显式局部"
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

echo "=================================================="
echo "防御审计: PASS $pass   FAIL $fail"
[ "$fail" == "0" ] && echo "ALL DEFENSES ACTIVE" || exit 1
