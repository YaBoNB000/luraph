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
		if [[ "$base" == luau_* ]]; then
			dialects="luau"
		else
			dialects="5.1 luau"
		fi
		for d in $dialects; do
			if [[ "$d" == "5.1" ]]; then interp=$LUA51; else interp=$LUAU; fi
			o1="$(timeout 60 "$interp" "$case" 2>&1)"; c1=$?
			for vmflag in "" "--vm"; do
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
echo "multiseed done: FAIL=$fail"
[ "$fail" -eq 0 ]
