#!/bin/bash
# 增量⑱ (选项B路线一) — 输入绑定/激活门 功能测试。
#
# 可独立运行 (bash tests/bind_gate_test.sh)，也可被 run_tests.sh source
# (此时复用调用方的 pass / fail / failed 计数器)。
#
# 交付模型（激活值如何到达产物）:
#   沙箱 Luau CLI 不转发命令行参数给 `...`、全局只读、无 io，
#   所以测试用两种真实交付形态包装产物后调用:
#     W1 包装函数:  local f = function(...) <产物> end; f("激活值", ...)
#     W2 loadstring: loadstring(<产物源码>)("激活值", ...)
#   生产环境同理: 加载器以第一个变长参数递送激活值
#   (loadstring(src)(key, ...); lua51 CLI 直接转发命令行参数)。
#
# 验证项:
#   G1 绑定产物 + 正确激活值 + 数据参数 == raw 运行 (W1 包装)
#   G2 绑定产物 + 正确激活值 + 无数据   == raw 默认路径
#   G3 错误激活值 -> 拒绝运行 (非零退出)
#   G4 缺失激活值 -> 拒绝运行 (非零退出)
#   G5 激活值字符串不出现在产物里 (字面量泄漏检查)
#   G6 未绑定对照: 不带 --bind-key 的产物无钥匙也能跑通 == raw
#   G7 多种子: 三个种子下绑定产物都只在正确钥匙下跑通
#   G8 loadstring 交付形态同样只在正确钥匙下跑通
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
SRC="$ROOT/tests/cases/bind_gate.lua"
KEY="luraph-2026"

TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUAU="$TOOLS_BIN/luau"

# standalone mode: own counters + summary
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	pass=0
	fail=0
	failed=()
	_STANDALONE=1
else
	_STANDALONE=0
fi

# BG_TMP (not TMP): when sourced from run_tests.sh the host script owns
# $TMP and its EXIT trap — clobbering either would leak its tempdir.
BG_TMP="$(mktemp -d)"
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	trap 'rm -rf "$BG_TMP"' EXIT
fi
TMP="$BG_TMP"

gate_fail() { # gate_fail <id> <msg>
	echo "FAIL [bind:$1] $2"
	fail=$((fail + 1)); failed+=("bind/$1")
}

run30() { timeout 30 "$@" 2>&1; }

# lua_quote "s" -> Lua double-quoted literal (test args are simple)
lua_quote() { printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"; }

# mkwrap <artifact> <out> [args...] — W1: wrap the artifact chunk in a
# function and call it with the given string args (activation first for
# bound artifacts; data only for raw references).
mkwrap() {
	local art="$1" out="$2"; shift 2
	local callargs=""
	local a
	for a in "$@"; do
		if [[ -n "$callargs" ]]; then callargs="$callargs, "; fi
		callargs="$callargs$(lua_quote "$a")"
	done
	{
		printf 'local _f = function(...)\n'
		cat "$art"
		printf '\nend\nreturn _f(%s)\n' "$callargs"
	} > "$out"
}

# mkls <artifact> <out> [args...] — W2: embed the artifact source in a
# safe-level long string, loadstring it, call with the args.
mkls() {
	local art="$1" out="$2"; shift 2
	local callargs=""
	local a
	for a in "$@"; do
		if [[ -n "$callargs" ]]; then callargs="$callargs, "; fi
		callargs="$callargs$(lua_quote "$a")"
	done
	# find a long-string level whose closer never occurs in the artifact
	local lvl=1
	while grep -qF "]$(printf '=%.0s' $(seq 1 $lvl))]" "$art"; do lvl=$((lvl + 1)); done
	local eq; eq="$(printf '=%.0s' $(seq 1 $lvl))"
	{
		printf 'local _s = [%s[\n' "$eq"
		cat "$art"
		printf '\n]%s]\nlocal _f = loadstring(_s)\nif not _f then error("loadstring failed") end\nreturn _f(%s)\n' "$eq" "$callargs"
	} > "$out"
}

# ---- build the bound artifact ------------------------------------
if ! "$TOOL" --preset v15 --dialect luau --bind-key "$KEY" --seed 42 "$SRC" "$TMP/bound.lua" 2>"$TMP/err.txt"; then
	gate_fail "build" "tool failed: $(head -1 "$TMP/err.txt")"
else
	# G1: correct key + data == raw with the same data
	mkwrap "$TMP/bound.lua" "$TMP/g1b.lua" "$KEY" hello World
	mkwrap "$SRC" "$TMP/g1r.lua" hello World
	b1="$(run30 "$LUAU" "$TMP/g1b.lua")"; r1=$?
	b2="$(run30 "$LUAU" "$TMP/g1r.lua")"; r2=$?
	if [[ "$r1" != "0" || "$r1" != "$r2" || "$b1" != "$b2" ]]; then
		gate_fail "g1" "correct key run diverges (exit $r1 vs $r2)"
		diff <(echo "$b2") <(echo "$b1") | head -6
	else
		pass=$((pass + 1))
	fi

	# G2: correct key, no data == raw default path
	mkwrap "$TMP/bound.lua" "$TMP/g2b.lua" "$KEY"
	mkwrap "$SRC" "$TMP/g2r.lua"
	b1="$(run30 "$LUAU" "$TMP/g2b.lua")"; r1=$?
	b2="$(run30 "$LUAU" "$TMP/g2r.lua")"; r2=$?
	if [[ "$r1" != "0" || "$r1" != "$r2" || "$b1" != "$b2" ]]; then
		gate_fail "g2" "correct-key default path diverges (exit $r1 vs $r2)"
		diff <(echo "$b2") <(echo "$b1") | head -6
	else
		pass=$((pass + 1))
	fi

	# G3: wrong key -> refused (non-zero exit; 124 = hang also counts
	# as "did not run", though a crash is the expected shape)
	mkwrap "$TMP/bound.lua" "$TMP/g3b.lua" "wrong-key" hello
	b1="$(run30 "$LUAU" "$TMP/g3b.lua")"; r1=$?
	if [[ "$r1" == "0" ]]; then
		gate_fail "g3" "WRONG KEY RAN THE PROGRAM: $b1"
	else
		pass=$((pass + 1))
	fi

	# G4: missing key -> refused
	mkwrap "$TMP/bound.lua" "$TMP/g4b.lua"
	b1="$(run30 "$LUAU" "$TMP/g4b.lua")"; r1=$?
	if [[ "$r1" == "0" ]]; then
		gate_fail "g4" "MISSING KEY RAN THE PROGRAM: $b1"
	else
		pass=$((pass + 1))
	fi

	# G5: the activation string must not appear in the output
	if grep -qF "$KEY" "$TMP/bound.lua"; then
		gate_fail "g5" "activation string leaked into the output"
	else
		pass=$((pass + 1))
	fi
fi

# G6: unbound control — same source without --bind-key runs keyless
if ! "$TOOL" --preset v15 --dialect luau --seed 42 "$SRC" "$TMP/unbound.lua" 2>"$TMP/err.txt"; then
	gate_fail "g6-build" "tool failed: $(head -1 "$TMP/err.txt")"
else
	b1="$(run30 "$LUAU" "$TMP/unbound.lua")"; r1=$?
	b2="$(run30 "$LUAU" "$SRC")"; r2=$?
	if [[ "$r1" != "$r2" || "$b1" != "$b2" ]]; then
		gate_fail "g6" "unbound control diverges (exit $r1 vs $r2)"
	else
		pass=$((pass + 1))
	fi
fi

# G7: multi-seed binding stability (correct key runs, wrong key dies)
for s in 1 7 4242; do
	if ! "$TOOL" --preset v15 --dialect luau --bind-key "$KEY" --seed "$s" "$SRC" "$TMP/ms_$s.lua" 2>"$TMP/err.txt"; then
		gate_fail "g7-build-$s" "tool failed: $(head -1 "$TMP/err.txt")"
		continue
	fi
	mkwrap "$TMP/ms_$s.lua" "$TMP/g7ok_$s.lua" "$KEY"
	mkwrap "$SRC" "$TMP/g7ref.lua"
	mkwrap "$TMP/ms_$s.lua" "$TMP/g7bad_$s.lua" "nope-$s"
	b1="$(run30 "$LUAU" "$TMP/g7ok_$s.lua")"; r1=$?
	b2="$(run30 "$LUAU" "$TMP/g7ref.lua")"; r2=$?
	b3="$(run30 "$LUAU" "$TMP/g7bad_$s.lua")"; r3=$?
	if [[ "$r1" != "0" || "$b1" != "$b2" ]]; then
		gate_fail "g7-ok-$s" "correct key diverges under seed $s"
	elif [[ "$r3" == "0" ]]; then
		gate_fail "g7-bad-$s" "wrong key RAN under seed $s"
	else
		pass=$((pass + 1))
	fi
done

# G8: loadstring delivery (canonical production shape)
if [[ -f "$TMP/bound.lua" ]]; then
	mkls "$TMP/bound.lua" "$TMP/g8ok.lua" "$KEY" xray
	mkls "$SRC" "$TMP/g8ref.lua" xray
	mkls "$TMP/bound.lua" "$TMP/g8bad.lua" "bad-key" xray
	b1="$(run30 "$LUAU" "$TMP/g8ok.lua")"; r1=$?
	b2="$(run30 "$LUAU" "$TMP/g8ref.lua")"; r2=$?
	b3="$(run30 "$LUAU" "$TMP/g8bad.lua")"; r3=$?
	if [[ "$r1" != "0" || "$b1" != "$b2" ]]; then
		gate_fail "g8-ok" "loadstring delivery diverges (exit $r1 vs $r2)"
		diff <(echo "$b2") <(echo "$b1") | head -6
	elif [[ "$r3" == "0" ]]; then
		gate_fail "g8-bad" "loadstring delivery RAN with wrong key"
	else
		pass=$((pass + 1))
	fi
fi

if [[ "$_STANDALONE" == "1" ]]; then
	echo "BIND GATE PASS: $pass   FAIL: $fail"
	if [[ "$fail" != "0" ]]; then
		echo "failed: ${failed[*]}"
		exit 1
	fi
	echo "ALL BIND GATES GREEN"
else
	# sourced: clean up our own tempdir (the host's EXIT trap is untouched)
	rm -rf "$BG_TMP"
fi
