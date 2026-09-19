#!/bin/bash
# ㊺ 忠实桩等价测试（--bind-env 绑定态的「正向」验证 + 读前写顺序守卫）。
#
# 背景: ㊸..㊹ 存在两处被掩盖的错误——
#   (1) 读前写: 段 B/C 排在 metavm 种子消费点之后 ⇒ ef1 恒以初值 0 被
#       消费 ⇒ 环境值绑定实为死代码，且补偿常数按「ef1 折出 env_exp」
#       设计 ⇒ 绑定态产物在目标运行时也解出垃圾、PD 挂死。
#   (2) 镜像错位: 门 (gate_c) 运行期形态 ((S·ec5+EN3·ec6+EN6·ec7)·ec8)
#       与 Rust 镜像分组 ((S+f_ty·ec5+f_ts·ec6)·ec7) 系数错位 ⇒ 即便
#       顺序修好，忠实环境折出的 ef1 也不等于镜像期望的 env_exp。
# ㊺ 两处同修。本测试给出此前完全缺失的**正向**断言:
#   (a) 忠实仿真（__tostring 元方法给 "x, y, z"、typeof 槽给 "Vector3"、
#       分量如实回传）⇒ 绑定态产物必须跑通且输出与源程序逐字一致。
#   (b) 顺序守卫: TSRC 里 ef1 的赋值点必须位于两处消费点（hqi 解掩种子、
#       metavm 元种子）之前——直接回归「读前写」这一整类错误。
#   (c) 前移消费存在性: hqi 解掩种子必须真引用 ef1（绑定折叠已前移到
#       引导期第一个解码步骤）。
#   (d) ㊻ pb 双消费存在性: hqi 解掩种子还必须引用 pb（原生性探针与
#       环境值折叠共同绑定第一个解码步骤）。
#
# 关键仿真约束（Luau CLI 限制，也即真实攻击者需达到的仿真深度）:
#   * getfenv(0) 恒返回真 _G（setfenv 影子不可见、_G 冻结、协程环境不
#     继承）⇒ 引导层经 GFE(0) 取的 tostring 是**真** tostring。桩对象的
#     字符串语义必须走 __tostring 元方法（真 tostring 会尊重它）。
#   * typeof 槽经模块表构造期裸标识符捕获 ⇒ 走块环境 ⇒ setfenv 的
#     env.typeof 桩可见（返回 "Vector3"，其余委托真 typeof）。
#
# 可独立运行: bash tests/faithful_stub_test.sh；也可被 run_tests.sh source。
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
SRC="$ROOT/tests/cases/basics.lua"

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
FS_TMP="$(mktemp -d)"
if [[ "$_STANDALONE" == "1" ]]; then trap 'rm -rf "$FS_TMP"' EXIT; fi
gf() { echo "FAIL [faithful:$1] $2"; fail=$((fail+1)); failed+=("faithful/$1"); }

EXP="$(timeout 30 "$LUAU" "$SRC" 2>&1)"

# ---- 生成绑定态产物（调试转储 TSRC 供顺序守卫使用）----
rm -f /tmp/vm_tsrc.lua
if ! LURAPH_VM_TSRC=1 "$TOOL" --preset v15 --dialect luau --bind-env roblox \
		--seed 42 "$SRC" "$FS_TMP/bound.lua" 2>"$FS_TMP/err"; then
	gf "build" "绑定态构建失败: $(head -1 "$FS_TMP/err")"
else
	# ---- (b) 顺序守卫: ef1 赋值点必须早于全部消费点 ----
	if [[ -f /tmp/vm_tsrc.lua ]]; then
		asg="$(grep -n 'ef1 = (((' /tmp/vm_tsrc.lua | head -1 | cut -d: -f1)"
		use_hqi="$(grep -n 'ef1 % 65536' /tmp/vm_tsrc.lua | head -1 | cut -d: -f1)"
		if [[ -z "$asg" || -z "$use_hqi" ]]; then
			gf "order-missing" "TSRC 缺 ef1 赋值/消费点（asg=$asg use=$use_hqi）"
		elif [[ "$asg" -lt "$use_hqi" ]]; then
			pass=$((pass+1))
		else
			gf "order" "ef1 赋值(行$asg)晚于消费(行$use_hqi)——读前写回归"
		fi
		# ---- (c) 前移消费存在性: hqi 解掩种子引用 ef1 ----
		if grep -q 'hqs, hqm, hqc = .*ef1' /tmp/vm_tsrc.lua; then
			pass=$((pass+1))
		else
			gf "forward" "hqi 解掩种子未引用 ef1（绑定折叠未前移）"
		fi
		# ---- (d) pb 双消费存在性（㊻）: hqi 解掩种子也引用 pb ----
		if grep -q 'hqs, hqm, hqc = .*pb' /tmp/vm_tsrc.lua; then
			pass=$((pass+1))
		else
			gf "pb-dual" "hqi 解掩种子未引用 pb（pb 双消费未前移）"
		fi
	else
		gf "tsrc" "TSRC 转储缺失（无法做顺序守卫）"
		pass=$((pass+1)) # 前移存在性无法验证，不重复计罚
		asg=; use_hqi=
		[[ -n "$asg" ]] || true
	fi
	rm -f /tmp/vm_tsrc.lua

	# ---- (a) 忠实仿真正向等价 ----
	python3 - "$FS_TMP/bound.lua" "$FS_TMP" <<'PYF'
import sys
src = open(sys.argv[1]).read()
W = sys.argv[2]
def wrap(s, name):
    for lvl in range(1, 5):
        cl = ']' + '='*lvl + ']'
        if cl not in s:
            return f'local {name} = [{ "="*lvl }[\n{s}\n]{ "="*lvl }]'
    raise RuntimeError("no level")
runner = wrap(src, 'BND') + """
local env = {}
for k, v in pairs(_G) do env[k] = v end
env._G = env
local rtypeof = typeof
local V3M = { __tostring = function(s)
  return s.X .. ", " .. s.Y .. ", " .. s.Z
end }
local V2M = {}
env.typeof = function(v)
  if rtypeof(v) == "table" then
    local mt = getmetatable(v)
    if mt == V3M then return "Vector3" end
    if mt == V2M then return "Vector2" end
  end
  return rtypeof(v)
end
env.Vector3 = { new = function(x, y, z) return setmetatable({X=x, Y=y, Z=z}, V3M) end }
env.Vector2 = { new = function(x, y) return setmetatable({X=x, Y=y}, V2M) end }
env.task = { defer = function() end }
local f = assert(loadstring(BND, "bound"))
setfenv(f, env)
local ok, err = pcall(f)
print("FAITHFUL-RUN:", ok, tostring(err):sub(1, 120))
"""
open(W + "/faithful.lua", "w").write(runner)
PYF
	out="$(timeout 30 "$LUAU" "$FS_TMP/faithful.lua" 2>&1)"; rc=$?
	prog_out="$(printf '%s\n' "$out" | grep -v '^FAITHFUL-RUN:')"
	if [[ "$rc" == "124" ]]; then
		gf "run" "忠实仿真超时（种子错钥/挂死形态）"
	elif echo "$out" | grep -qE "FAITHFUL-RUN:[[:space:]]*true" && [[ "$prog_out" == "$EXP" ]]; then
		pass=$((pass+1))
	else
		gf "run" "忠实仿真未跑通或输出不一致 (rc=$rc)"
	fi
fi

if [[ "$_STANDALONE" == "1" ]]; then
	echo "FAITHFUL PASS: $pass   FAIL: $fail"
	[[ "$fail" != "0" ]] && { echo "failed: ${failed[*]}"; exit 1; }
	echo "ALL FAITHFUL GREEN"
else
	rm -rf "$FS_TMP"
fi
