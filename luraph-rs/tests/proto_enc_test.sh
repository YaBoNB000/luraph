#!/bin/bash
# 增量㉒ (选项B — B-2 碎片即用即毁) 测试：原型加密集散。
#
# 防御目标（R006 壁垒①）：进程生命周期内任何时刻都不存在完整解码态
# 程序——boot 解析出的原型表立即由 ENC 碎片重编码为密文并销毁平表
# （PF = nil）；每次调用经 DDEC 碎片按帧解码出新表，帧退即毁。闭包只
# 捕获密文原型（makefn(pf, ...) + E.k 子原型图）。
#
# 检查项：
#   (1) pe-enc   模板形态：ENC/DDEC 接线齐全、PF 销毁、入口走 MPF、
#                makefn 不再查 PF 平表、Closure 经 E.k 取子原型；
#   (2) pe-frags HQ 碎片数 = 43 指令(42 基类+NopA 别名) + 4 重入 + 4 执行器
#                + BSS + DDEC + ENC = 54；
#   (3) pe-run   嵌套闭包程序运行等价（3 种子）；
#   (4) pe-const 常量往返：小数/大整数/布尔/含转义字符串逐字还原；
#   (5) pe-rec   深递归 + 闭包计数器运行等价。
# 可独立运行，也可被 run_tests.sh source（复用计数器）。
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"

TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUAU="$TOOLS_BIN/luau"

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	pass=0; fail=0; failed=()
	_STANDALONE=1
else
	_STANDALONE=0
fi
PE_TMP="$(mktemp -d)"
if [[ "$_STANDALONE" == "1" ]]; then trap 'rm -rf "$PE_TMP"' EXIT; fi

gf() { echo "FAIL [protoenc:$1] $2"; fail=$((fail+1)); failed+=("protoenc/$1"); }

# ---------- 用例 ----------
cat > "$PE_TMP/nested.lua" <<'EOF'
local function outer(x)
  local function inner(y)
    return x + y * 2
  end
  return inner(x) + inner(10)
end
local t = {}
for i = 1, 6 do t[i] = outer(i) end
print(table.concat(t, ","))
EOF

cat > "$PE_TMP/consts.lua" <<'EOF'
local pi = 3.14159
local big = 1000000000000000
local neg = -0.000125
local s1 = "tab\tnl\nquote\"end"
local s2 = ""
local b1, b2 = true, false
print(pi, big, neg, pi + neg, s1, #s1, s2 == "", b1, b2, 2 ^ 52 + 1, 0x7FFFFFFF)
EOF

cat > "$PE_TMP/rec.lua" <<'EOF'
local function fib(n)
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end
local function make_counter(start)
  local c = start
  return function(step)
    c = c + step
    return c
  end
end
local ctr = make_counter(100)
print(fib(14), ctr(7), ctr(11), ctr(1))
EOF

# ---------- (1) 模板形态 ----------
# （LURAPH_VM_TSRC 写死输出到 /tmp/vm_tsrc.lua）
rm -f /tmp/vm_tsrc.lua
if LURAPH_VM_TSRC=1 "$TOOL" --preset v15 --dialect luau --seed 42 \
	"$PE_TMP/nested.lua" "$PE_TMP/nested.lua.out" 2>"$PE_TMP/err"; then
	ts=/tmp/vm_tsrc.lua
	ok=1
	# 注：E.k[b + 1]（Closure 取子原型）在加密碎片内，TSRC 不可见，
	# 其存在性由 (3)/(5) 的嵌套闭包运行等价间接验证
	for pat in "MPF = ENCF(" "PF = nil" "DDEC = HW\[206\](" "newE = function(pf" \
		"makefn(pf, V, upsf, S)" "return run(MPF"; do
		if ! grep -q "$pat" "$ts"; then ok=0; gf "enc" "模板缺少形态: $pat"; fi
	done
	# 平表残留引用必须消失
	for pat in "PF\[#FN\]" "PF\[idx\]"; do
		if grep -q "$pat" "$ts"; then ok=0; gf "enc" "模板残留平表引用: $pat"; fi
	done
	[[ "$ok" == "1" ]] && pass=$((pass+1))
else
	gf "enc" "构建失败: $(head -1 "$PE_TMP/err")"
fi

# ---------- (2) 碎片计数 ----------
n_hq=$(grep -c "HQ\[" /tmp/vm_tsrc.lua 2>/dev/null || echo 0)
if [[ "$n_hq" == "54" ]]; then
	pass=$((pass+1))
else
	gf "frags" "HQ 碎片数 $n_hq != 54（43 指令+4 重入+4 执行器+BSS+DDEC+ENC）"
fi

# ---------- (3) 嵌套闭包运行等价（3 种子） ----------
o1="$(timeout 30 "$LUAU" "$PE_TMP/nested.lua" 2>&1)"; r1=$?
ok=1
for seed in 1 777 31337; do
	"$TOOL" --preset v15 --dialect luau --seed "$seed" \
		"$PE_TMP/nested.lua" "$PE_TMP/n.$seed.lua" >/dev/null 2>&1
	o2="$(timeout 30 "$LUAU" "$PE_TMP/n.$seed.lua" 2>&1)"; r2=$?
	if [[ "$r1" != "$r2" || "$o1" != "$o2" ]]; then
		ok=0; gf "run" "nested seed=$seed 不等价 (rc $r1 vs $r2)"
	fi
done
[[ "$ok" == "1" ]] && pass=$((pass+1))

# ---------- (4) 常量往返 ----------
o1="$(timeout 30 "$LUAU" "$PE_TMP/consts.lua" 2>&1)"; r1=$?
"$TOOL" --preset v15 --dialect luau --seed 42 \
	"$PE_TMP/consts.lua" "$PE_TMP/consts.out.lua" >/dev/null 2>&1
o2="$(timeout 30 "$LUAU" "$PE_TMP/consts.out.lua" 2>&1)"; r2=$?
if [[ "$r1" == "$r2" && "$o1" == "$o2" ]]; then
	pass=$((pass+1))
else
	gf "const" "常量往返失真 (rc $r1 vs $r2): '$o1' vs '$o2'"
fi

# ---------- (5) 深递归 + 闭包计数器 ----------
o1="$(timeout 30 "$LUAU" "$PE_TMP/rec.lua" 2>&1)"; r1=$?
"$TOOL" --preset v15 --dialect luau --seed 999999 \
	"$PE_TMP/rec.lua" "$PE_TMP/rec.out.lua" >/dev/null 2>&1
o2="$(timeout 30 "$LUAU" "$PE_TMP/rec.out.lua" 2>&1)"; r2=$?
if [[ "$r1" == "$r2" && "$o1" == "$o2" ]]; then
	pass=$((pass+1))
else
	gf "rec" "递归/闭包失真 (rc $r1 vs $r2): '$o1' vs '$o2'"
fi

if [[ "$_STANDALONE" == "1" ]]; then
	echo "PROTO ENC PASS: $pass   FAIL: $fail"
	[[ "$fail" != "0" ]] && { echo "failed: ${failed[*]}"; exit 1; }
	echo "ALL PROTO ENC GREEN"
else
	rm -rf "$PE_TMP"
fi
