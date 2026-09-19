#!/bin/bash
# 增量㉑ 环境绑定测试（--bind-env）。
# 绑定态产物只在目标运行时（Roblox）可加载；通用分析沙箱（luau CLI）在
# 加载期即失败（原语槽引用 Vector3/Vector2/task，构造即 index nil）。
# 故绑定态产物不进通用运行等价矩阵，只做两项验证：
#   (a) 语法校验通过（luau-compile）；
#   (b) CLI 加载失败（绑定生效——这正是防御目的）。
# 同时验证未绑定产物不受影响（零参数直接可运行）。
# 可独立运行，也可被 run_tests.sh source（复用计数器）。
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
SRC="$ROOT/tests/cases/basics.lua"

TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUAU="$TOOLS_BIN/luau"
LUAUC="$TOOLS_BIN/luau-compile"

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	pass=0; fail=0; failed=()
	_STANDALONE=1
else
	_STANDALONE=0
fi
BG_TMP="$(mktemp -d)"
if [[ "$_STANDALONE" == "1" ]]; then trap 'rm -rf "$BG_TMP"' EXIT; fi

gf() { echo "FAIL [envbind:$1] $2"; fail=$((fail+1)); failed+=("envbind/$1"); }

# 生成绑定态产物
if ! "$TOOL" --preset v15 --dialect luau --bind-env roblox --seed 42 "$SRC" "$BG_TMP/bound.lua" 2>"$BG_TMP/err"; then
	gf "build" "tool failed: $(head -1 "$BG_TMP/err")"
else
	# (a) 语法校验通过
	if "$LUAUC" "$BG_TMP/bound.lua" >/dev/null 2>&1; then
		pass=$((pass+1))
	else
		gf "syntax" "绑定产物语法校验失败"
	fi
	# (b) CLI 加载失败（绑定生效）
	timeout 5 "$LUAU" "$BG_TMP/bound.lua" >/dev/null 2>&1; rc=$?
	if [[ "$rc" != "0" ]]; then
		pass=$((pass+1))
	else
		gf "leak" "绑定产物在通用沙箱竟然跑起来了（绑定失效）"
	fi
	# (c) 产物含目标运行时 API 引用
	if grep -q "Vector3\|Vector2\|task\." "$BG_TMP/bound.lua"; then
		pass=$((pass+1))
	else
		gf "noref" "绑定产物未嵌入目标运行时 API 引用"
	fi
fi

# (d) ㊸ 环境绑定去中心化形态：绑定态 = 模块表槽捕获（Vector3.new 等，
# 混入原语槽族）+ 引导三段槽位间接调用 + ef1 折叠进元钥匙流；
# 未绑定态只保留无害的 ef1 声明，无折叠消费。
rm -f /tmp/vm_tsrc.lua
LURAPH_VM_TSRC=1 "$TOOL" --preset v15 --dialect luau --bind-env roblox --seed 42 "$SRC" "$BG_TMP/bound2.lua" >/dev/null 2>&1
if grep -q "ef1%\|ef1 %" /tmp/vm_tsrc.lua 2>/dev/null && grep -q "Vector3\.new" "$BG_TMP/bound2.lua"; then
	pass=$((pass+1))
else
	gf "envval" "绑定态缺少环境值折叠消费（ef1 钥匙流项 / 槽捕获）"
fi
rm -f /tmp/vm_tsrc.lua
LURAPH_VM_TSRC=1 "$TOOL" --preset v15 --dialect luau --seed 42 "$SRC" "$BG_TMP/unbound2.lua" >/dev/null 2>&1
if grep -q "ef1%\|ef1 %" /tmp/vm_tsrc.lua 2>/dev/null; then
	gf "envval-leak" "未绑定产物混入环境值折叠消费"
else
	pass=$((pass+1))
fi

# 未绑定对照：零参数直接可运行（无回归）
"$TOOL" --preset v15 --dialect luau --seed 42 "$SRC" "$BG_TMP/unbound.lua" 2>/dev/null
o1="$(timeout 30 "$LUAU" "$SRC" 2>&1)"; r1=$?
o2="$(timeout 30 "$LUAU" "$BG_TMP/unbound.lua" 2>&1)"; r2=$?
if [[ "$r1" == "$r2" && "$o1" == "$o2" ]]; then
	pass=$((pass+1))
else
	gf "unbound" "未绑定产物回归（rc $r1 vs $r2）"
fi

if [[ "$_STANDALONE" == "1" ]]; then
	echo "ENV BIND PASS: $pass   FAIL: $fail"
	[[ "$fail" != "0" ]] && { echo "failed: ${failed[*]}"; exit 1; }
	echo "ALL ENV BIND GREEN"
else
	rm -rf "$BG_TMP"
fi
