#!/bin/bash
# Multi-seed regression (M4 续期起为 VM 改动的必跑项):
# the official matrix (run_tests.sh) is pinned to --seed 42, but the
# opcode permutation / operand slot permutation is random PER BUILD,
# so seed-only-at-42 misses permutation-surface regressions.
#
# Runs every corpus case x seeds x both dialects, non-VM and VM phases,
# including the 5.1-target-on-luau cross check. A timeout is a FAIL.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/lua51" ] || [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUA51="$TOOLS_BIN/lua51"
LUAU="$TOOLS_BIN/luau"
SEEDS="${1:-1 7 4242 31337 999999}"

fail=0
for seed in $SEEDS; do
	for case in "$ROOT"/tests/cases/*.lua; do
		base="$(basename "$case" .lua)"
		# ㉖: 产品线 Luau-only（5.1 相位移除）
		dialects="luau"
		for d in $dialects; do
			if [[ "$d" == "5.1" ]]; then interp=$LUA51; else interp=$LUAU; fi
			o1="$(timeout 60 "$interp" "$case" 2>&1)"; c1=$?
			# 增量⑩起: v15 profile joins the seed sweep (Luau-only). The
			# per-build key-fragment recipes are seed-shaped — seed-42-only
			# coverage missed an anchor-length bug.
			vmflags=("" "--vm")
			[[ "$d" == "luau" ]] && vmflags+=("--preset v15")
			for vmflag in "${vmflags[@]}"; do
				if ! timeout 60 "$TOOL" $vmflag --dialect "$d" --seed "$seed" "$case" /tmp/ms_out.lua 2>/dev/null; then
					echo "FAIL [tool] $base $d ${vmflag:-nv} seed=$seed"; fail=$((fail+1)); continue
				fi
				o2="$(timeout 60 "$interp" /tmp/ms_out.lua 2>&1)"; c2=$?
				if [[ "$c1" != "$c2" || "$o1" != "$o2" ]]; then
					echo "FAIL [run]   $base $d ${vmflag:-nv} seed=$seed (rc $c1 vs $c2)"; fail=$((fail+1))
				fi
				if [[ "$d" == "5.1" ]]; then
					o3="$(timeout 60 "$LUAU" /tmp/ms_out.lua 2>&1)"; c3=$?
					o4="$(timeout 60 "$LUAU" "$case" 2>&1)"; c4=$?
					if [[ "$c3" != "$c4" || "$o3" != "$o4" ]]; then
						echo "FAIL [cross] $base seed=$seed ${vmflag:-nv}"; fail=$((fail+1))
					fi
				fi
			done
		done
	done
done
# 增量⑱ (选项B路线一) — activation-gate seed sweep. The gate rides the
# v15 boot chain; per-build the AK fold, the mix key, and the key-table
# recipes are all seed-shaped, so sweep bound builds across seeds:
# correct key must reproduce the raw default path, wrong key must die.
BGATE="$ROOT/tests/cases/bind_gate.lua"
if [[ -f "$BGATE" ]]; then
	for seed in $SEEDS; do
		if ! timeout 60 "$TOOL" --preset v15 --dialect luau --bind-key "luraph-2026" \
			--seed "$seed" "$BGATE" /tmp/ms_bind.lua 2>/dev/null; then
			echo "FAIL [bind:tool] seed=$seed"; fail=$((fail+1)); continue
		fi
		o1="$(timeout 60 "$LUAU" "$BGATE" 2>&1)"; c1=$?
		printf 'local _f=function(...)\n' > /tmp/ms_bind_ok.lua
		cat /tmp/ms_bind.lua >> /tmp/ms_bind_ok.lua
		printf '\nend\nreturn _f("luraph-2026")\n' >> /tmp/ms_bind_ok.lua
		printf 'local _f=function(...)\n' > /tmp/ms_bind_bad.lua
		cat /tmp/ms_bind.lua >> /tmp/ms_bind_bad.lua
		printf '\nend\nreturn _f("wrong-%s")\n' "$seed" >> /tmp/ms_bind_bad.lua
		o2="$(timeout 60 "$LUAU" /tmp/ms_bind_ok.lua 2>&1)"; c2=$?
		o3="$(timeout 60 "$LUAU" /tmp/ms_bind_bad.lua 2>&1)"; c3=$?
		if [[ "$c2" != "0" || "$o2" != "$o1" ]]; then
			echo "FAIL [bind:ok] seed=$seed correct-key run diverges (rc $c1 vs $c2)"; fail=$((fail+1))
		fi
		if [[ "$c3" == "0" ]]; then
			echo "FAIL [bind:bad] seed=$seed WRONG KEY RAN"; fail=$((fail+1))
		fi
	done
fi

echo "multiseed done: FAIL=$fail"
[ "$fail" -eq 0 ]
