//! L6 VM — interpreter template generation.
//!
//! Emits the interpreter as Lua source (dialect-neutral, 5.1+Luau
//! compatible: no bitops, no `//`, no goto), then the project's own
//! parser turns it into an AST that runs through the full obfuscation
//! pipeline (mangle/flatten/strings/numbers/body/antidbg). Per build:
//! opcode codes are a random permutation (shared with the compiler)
//! and the dispatch branch order is shuffled.
//!
//! VM model:
//! - a VM function value is a Lua CLOSURE (natively callable, so
//!   pcall / coroutine.create / wrap / table.sort etc. all work)
//! - each call: the closure body sets up a fresh value array and runs
//!   the shared dispatch loop; results come back as a (array, count)
//! - upvalue = shared cell table { v = <creating frame's V array>,
//!   i = <slot> }; the creating frame materializes every upvalue
//!   symbol in its own V (compiler invariant)
//! - coroutines: `coroutine.yield` is a native global call inside the
//!   dispatch loop; the frame locals (V, pc, ...) are retained on the
//!   coroutine's stack across yield/resume

use crate::rng::Rng;
use crate::vmgen::isa::{OpMap, N_OPS};

/// Opcode names in base order (must match isa::op_index).
const OP_NAMES: [&str; N_OPS] = [
	"Jmp", "Jf", "Jt", "LoadNil", "LoadK", "Move", "Add", "Sub", "Mul", "Div",
	"Mod", "Pow", "Concat", "Unm", "Not", "Len", "Lt", "Le", "Gt", "Ge", "Eq",
	"Ne", "Idiv", "NewTab", "GetTab", "SetTab", "TabN", "CallT", "Closure",
	"Call", "VarArgTab", "VarArgC", "VarArgTabN", "GetGlobal", "SetGlobal",
	"GetUp", "SetUp", "Return", "Nop", "CallE", "CallM",
];

/// The dispatch body of each opcode (referencing locals: oc, a, b, c, d,
/// pc, V, C, lastn, vargs, vargc, plus helpers mget/makefn and locals
/// G/U/FLOOR).
fn branch_code(name: &str) -> String {
	match name {
	
		"Jmp" => "pc = b".to_string(),
		"Jf" => "if not V[a + 1] then pc = b end".to_string(),
		"Jt" => "if V[a + 1] then pc = b end".to_string(),
		"LoadNil" => "for i = 1, b do V[a + i] = nil end".to_string(),
		"LoadK" => "V[a + 1] = C[b + 1]".to_string(),
		"Move" => "V[a + 1] = V[b + 1]".to_string(),
		"Add" => bin_op_code("__add", "arithmetic", "x + y"),
		"Sub" => bin_op_code("__sub", "arithmetic", "x - y"),
		"Mul" => bin_op_code("__mul", "arithmetic", "x * y"),
		"Div" => bin_op_code("__div", "arithmetic", "x / y"),
		"Mod" => bin_op_code("__mod", "arithmetic", "x % y"),
		"Pow" => bin_op_code("__pow", "arithmetic", "x ^ y"),
		"Concat" => "local x = V[b + 1]; local y = V[c + 1]; local tx = type(x); local ty = type(y); if (tx == 'number' or tx == 'string') and (ty == 'number' or ty == 'string') then V[a + 1] = x .. y else local f = mget(x, '__concat') or mget(y, '__concat'); if f then V[a + 1] = f(x, y) else error('attempt to perform concatenation on a ' .. tx .. ' value', 0) end end".to_string(),
		"Unm" => "local x = V[b + 1]; if type(x) == 'number' then V[a + 1] = -x else local f = mget(x, '__unm'); if f then V[a + 1] = f(x) else error('attempt to perform arithmetic on a ' .. type(x) .. ' value', 0) end end".to_string(),
		"Not" => "V[a + 1] = not V[b + 1]".to_string(),
		"Len" => "local x = V[b + 1]; local f = HAS_LEN_META and mget(x, '__len'); if f then V[a + 1] = f(x) else V[a + 1] = #x end".to_string(),
		"Lt" => cmp_code("x < y", "__lt", false),
		"Le" => cmp_code("x <= y", "__le", false),
		"Gt" => cmp_code("x > y", "__lt", true),
		"Ge" => cmp_code("x >= y", "__le", true),
		"Eq" => "local x = V[b + 1]; local y = V[c + 1]; local tx = type(x); local ty = type(y); if tx == ty and (tx == 'number' or tx == 'string' or tx == 'boolean' or tx == 'nil') then V[a + 1] = x == y else local f = mget(x, '__eq') or mget(y, '__eq'); if f then V[a + 1] = f(x, y) else V[a + 1] = x == y end end".to_string(),
		"Ne" => "local x = V[b + 1]; local y = V[c + 1]; local tx = type(x); local ty = type(y); local eqv; if tx == ty and (tx == 'number' or tx == 'string' or tx == 'boolean' or tx == 'nil') then eqv = x == y else local f = mget(x, '__eq') or mget(y, '__eq'); if f then eqv = f(x, y) else eqv = x == y end end; V[a + 1] = not eqv".to_string(),
		"Idiv" => "V[a + 1] = FLOOR(V[b + 1] / V[c + 1])".to_string(),
		"NewTab" => "V[a + 1] = {}".to_string(),
"GetTab" => "local t = V[b + 1]; local k = V[c + 1]; local r; if type(t) == 'table' then r = rawget(t, k); if r == nil then local f = mget(t, '__index'); if type(f) == 'function' then r = f(t, k) elseif f ~= nil then r = f[k] end end else local mt = getmetatable(t); if mt and mt['__index'] ~= nil then local f = mt['__index']; if type(f) == 'function' then r = f(t, k) else r = f[k] end else error('attempt to index a ' .. type(t) .. ' value', 0) end end; V[a + 1] = r".to_string(),
"SetTab" => "local t = V[a + 1]; local k = V[b + 1]; local v = V[c + 1]; if type(t) ~= 'table' then local mt = getmetatable(t); local f = mt and mt['__newindex']; if type(f) == 'function' then f(t, k, v) elseif f ~= nil then rawset(f, k, v) else error('attempt to index a ' .. type(t) .. ' value', 0) end else if rawget(t, k) == nil then local f = mget(t, '__newindex'); if type(f) == 'function' then f(t, k, v) elseif f ~= nil then rawset(f, k, v) else rawset(t, k, v) end else rawset(t, k, v) end end".to_string(),
		"TabN" => "local t = V[a + 1]; local n = V[b + 1]; t[n + 1] = V[c + 1]; V[b + 1] = n + 1".to_string(),
		"CallT" => "local f = V[a + 1]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = math.floor(d / 2); local tail = d % 2 == 1; local ntail = tail and V[a + nfixed + 2] or 0; local nargs = nfixed + off + ntail; local args = {}; if off == 1 then args[1] = f end; for i = 1, nfixed + ntail do if tail and i > nfixed then args[off + i] = V[a + i + 2] else args[off + i] = V[a + i + 1] end end; local t = V[b + 1]; local n = V[c + 1]; local out, nout = callcap(fn, args, nargs); for i = 1, nout do t[n + i] = out[i] end; V[c + 1] = n + nout".to_string(),
		"Closure" => "V[a + 1] = makefn(b + 1, V, ups)".to_string(),
		"Call" => "local base = a + 1; local f = V[base]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nargs = b + off; local args = {}; if off == 1 then args[1] = f end; for i = 1, b do args[off + i] = V[base + i] end; if d == 1 then for i = 1, vargc do args[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local out, nout = callcap(fn, args, nargs); local nres = c; lastbase = a + 1; lastn = nout; local wn = nout; if nres ~= 255 and nres > wn then wn = nres end; for i = 1, wn do V[base + i] = out[i] end".to_string(),
		"CallE" => "local base = a + 1; local f = V[base]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = b; local varg = d % 2 == 1; local tail = d >= 2; local nargs = nfixed + off; if tail then nargs = nargs + V[base + nfixed + 1] end; local args = {}; if off == 1 then args[1] = f end; for i = 1, (tail and (nfixed + V[base + nfixed + 1]) or nfixed) do if tail and i > nfixed then args[off + i] = V[base + i + 1] else args[off + i] = V[base + i] end end; if varg then for i = 1, vargc do args[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local out, nout = callcap(fn, args, nargs); for i = 1, nout do V[base + i] = out[i] end; V[base] = nout".to_string(),
		"CallM" => "local base = a + 1; local f = V[base]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = b; local ntail = V[base + nfixed + 1]; local nargs = nfixed + ntail + off; local args = {}; if off == 1 then args[1] = f end; for i = 1, nfixed + ntail do if i <= nfixed then args[off + i] = V[base + i] else args[off + i] = V[base + i + 1] end end; if d == 1 then for i = 1, vargc do args[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local out, nout = callcap(fn, args, nargs); local nres = c; lastbase = a + 1; lastn = nout; local wn = nout; if nres ~= 255 and nres > wn then wn = nres end; for i = 1, wn do V[base + i] = out[i] end".to_string(),
		"VarArgTab" => "local t = {}; for i = 1, vargc do t[i] = vargs[i] end; V[a + 1] = t".to_string(),
		"VarArgC" => "V[a + 1] = vargc".to_string(),
		"VarArgTabN" => "local t = V[a + 1]; local n = V[b + 1]; for i = 1, vargc do t[n + i] = vargs[i] end; V[b + 1] = n + vargc".to_string(),
		"GetGlobal" => "V[a + 1] = G[C[b + 1]]".to_string(),
		"SetGlobal" => "G[C[b + 1]] = V[a + 1]".to_string(),
		"GetUp" => "local u = ups[b + 1]; V[a + 1] = u.v[u.i]".to_string(),
		"SetUp" => "local u = ups[a + 1]; u.v[u.i] = V[b + 1]".to_string(),
		"Return" => "local out = {}; local n = b; local total; if n == 255 then local pre = d; for i = 1, pre do out[i] = V[a + i] end; if c == 1 then for i = 1, vargc do out[pre + i] = vargs[i] end; total = pre + vargc else for i = 1, lastn do out[pre + i] = V[lastbase + i] end; total = pre + lastn end else for i = 1, n do out[i] = V[a + i] end; total = n end; return out, total".to_string(),
		"Nop" => "".to_string(),
		_ => panic!("unknown opcode name {name}"),
	}
}

fn bin_op_code(mm: &str, what: &str, expr: &str) -> String {
	format!(
		"local x = V[b + 1]; local y = V[c + 1]; if type(x) == 'number' and type(y) == 'number' then V[a + 1] = {expr} else local f = mget(x, '{mm}') or mget(y, '{mm}'); if f then V[a + 1] = f(x, y) else error('attempt to perform {what} on a ' .. type(x) .. ' value', 0) end end"
	)
}

fn cmp_code(native: &str, mm: &str, swapped: bool) -> String {
	let call = if swapped { "f(y, x)" } else { "f(x, y)" };
	format!(
		"local x = V[b + 1]; local y = V[c + 1]; if type(x) == 'number' and type(y) == 'number' then V[a + 1] = {native} elseif type(x) == 'string' and type(y) == 'string' then V[a + 1] = {native} else local f = mget(x, '{mm}') or mget(y, '{mm}'); if f then V[a + 1] = {call} else error('attempt to compare ' .. type(x) .. ' with ' .. type(y), 0) end end"
	)
}

/// Generate the interpreter source. `fns` = number of bytecode string
/// parameters (functions). The final entry call arguments are left as
/// a placeholder list of the parameter names (the caller appends the
/// bytecode literals as the actual call).
/// Random dead-instruction (Nop) body: harmless arithmetic on the
/// always-numeric operands, no side effects (picked per build).
fn nop_body(rng: &mut Rng) -> String {
	match rng.int(0, 2) {
		0 => "local _ = a + b".to_string(),
		1 => "local _ = c * d".to_string(),
		_ => "local _ = a * c + b".to_string(),
	}
}

/// M5 random decision-tree dispatch. Threshold splits on the wire codes
/// with a random pivot + random comparison form; leaves are equality
/// tests; at the depth cap a flat if/elseif bottom is emitted, so the
/// visible nesting stays in the 2~4 layer band. Per-build shape.
fn gen_dispatch_tree(
	items: &[(usize, u8)],
	body_of: &dyn Fn(&str) -> String,
	rng: &mut Rng,
	depth: u32,
) -> String {
	let n = items.len();
	if n == 1 {
		let name = OP_NAMES[items[0].0];
		return format!("if oc == OC.{} then\n{}\nend", name, body_of(name));
	}
	if depth >= 3 {
		let mut v: Vec<(usize, u8)> = items.to_vec();
		rng.shuffle(&mut v);
		let mut s = String::new();
		for (i, (idx, _)) in v.iter().enumerate() {
			let name = OP_NAMES[*idx];
			if i == 0 {
				s.push_str(&format!("if oc == OC.{} then\n{}\n", name, body_of(name)));
			} else {
				s.push_str(&format!(
					"elseif oc == OC.{} then\n{}\n",
					name,
					body_of(name)
				));
			}
		}
		s.push_str("end");
		return s;
	}
	let mut sc: Vec<u8> = items.iter().map(|&(_, c)| c).collect();
	sc.sort_unstable();
	let mut gaps: Vec<(u8, u8)> = Vec::new();
	for w in 1..sc.len() {
		if sc[w] > sc[w - 1] + 1 {
			gaps.push((sc[w - 1], sc[w]));
		}
	}
	if !gaps.is_empty() {
		let (lo, hi) = gaps[rng.int(0, (gaps.len() - 1) as i64) as usize];
		let p: u8 = if hi as i64 - lo as i64 == 2 {
			lo + 1
		} else {
			rng.int(lo as i64 + 1, hi as i64 - 1) as u8
		};
		let (left, right): (Vec<(usize, u8)>, Vec<(usize, u8)>) =
			items.iter().cloned().partition(|&(_, c)| c < p);
		let left_s = gen_dispatch_tree(&left, body_of, rng, depth + 1);
		let right_s = gen_dispatch_tree(&right, body_of, rng, depth + 1);
		if rng.int(0, 1) == 0 {
			format!("if oc < {} then\n{}\nelse\n{}\nend", p, left_s, right_s)
		} else {
			format!("if oc > {} then\n{}\nelse\n{}\nend", p - 1, right_s, left_s)
		}
	} else {
		// all wire codes adjacent: rank-split with a <= threshold
		let k = rng.int(1, (sc.len() - 1) as i64) as usize;
		let p = sc[k - 1];
		let (left, right): (Vec<(usize, u8)>, Vec<(usize, u8)>) =
			items.iter().cloned().partition(|&(_, c)| c <= p);
		let left_s = gen_dispatch_tree(&left, body_of, rng, depth + 1);
		let right_s = gen_dispatch_tree(&right, body_of, rng, depth + 1);
		if rng.int(0, 1) == 0 {
			format!("if oc <= {} then\n{}\nelse\n{}\nend", p, left_s, right_s)
		} else {
			format!("if oc < {} then\n{}\nelse\n{}\nend", p + 1, left_s, right_s)
		}
	}
}

pub fn generate(map: &OpMap, slot_perm: &[u8; 4], rng: &mut Rng, n_fns: usize) -> String {
	// opcode table with per-build random codes
	let mut oc_items = Vec::new();
	for (i, name) in OP_NAMES.iter().enumerate() {
		oc_items.push(format!("{name} = {}", map.to_wire[i]));
	}
	let oc_table = format!("local OC = {{{}}}", oc_items.join(", "));

	// M5: per-build Nop (dead instruction) body
	let nop = nop_body(rng);
	let body_of = |name: &str| -> String {
		if name == "Nop" {
			nop.clone()
		} else {
			branch_code(name)
		}
	};
	// randomized decision-tree dispatch over ALL opcodes (Nop included —
	// it carries the dead-instruction padding)
	let items: Vec<(usize, u8)> = (0..N_OPS).map(|i| (i, map.to_wire[i])).collect();
	let branches = gen_dispatch_tree(&items, &body_of, rng, 0);

	// M5 hub randomization: map stream operand slots back to a/b/c/d
	// (slot_perm[sl] = operand index in stream slot sl)
	if std::env::var("LURAPH_VM_DBG").is_ok() {
		eprintln!("[gen] slot_perm={:?}", slot_perm);
	}
	let mut pos_of = [1u8; 4];
	for (sl, &op_idx) in slot_perm.iter().enumerate() {
		pos_of[op_idx as usize] = sl as u8 + 1;
	}
	let fetch = format!(
		"local t1, p2 = r16(B, pc); pc = p2\n      local t2, p3 = r16(B, pc); pc = p3\n      local t3, p4 = r16(B, pc); pc = p4\n      local t4, p5 = r16(B, pc); pc = p5\n      local a = t{}; local b = t{}; local c = t{}; local d = t{}",
		pos_of[0], pos_of[1], pos_of[2], pos_of[3]
	);

	// parameter list F1..Fn
	let mut params = Vec::new();
	for i in 1..=n_fns {
		params.push(format!("F{i}"));
	}
	let params = params.join(", ");

	format!(
		r#"local VM = function({params})
  {oc_table}
  local FN = {{{params}}}
  local PF = {{}}
  local function u16(B, p)
    return string.byte(B, p) + string.byte(B, p + 1) * 256
  end
  -- 7-bit-chunk varint (M5): b1 < 128 -> 1 byte; else 2 bytes
  -- (b1-128) + b2*128. No bitops (5.1 template constraint).
  local function r16(B, p)
    local b1 = string.byte(B, p)
    if b1 < 128 then
      return b1, p + 1
    end
    return (b1 - 128) + string.byte(B, p + 1) * 128, p + 2
  end
  local function parse(s)
    local p = 1
    local nregs = u16(s, p); p = p + 2
    local nparams = u16(s, p); p = p + 2
    local vararg = string.byte(s, p); p = p + 1
    local nups = u16(s, p); p = p + 2
    local upsrc = {{}}
    for i = 1, nups do upsrc[i] = u16(s, p); p = p + 2 end
    local nconst = u16(s, p); p = p + 2
    local C = {{}}
    for i = 1, nconst do
      local t = string.byte(s, p); p = p + 1
      if t == 0 then
        C[i] = nil
      elseif t == 1 then
        C[i] = string.byte(s, p) == 1; p = p + 1
      else
        local l = u16(s, p); p = p + 2
        local x = string.sub(s, p, p + l - 1); p = p + l
        if t == 2 then C[i] = tonumber(x) else C[i] = x end
      end
    end
    return {{ B = string.sub(s, p), nregs = nregs, nparams = nparams, vararg = vararg, upsrc = upsrc, C = C }}
  end
  for i = 1, #FN do PF[i] = parse(FN[i]) end
  -- writable global environment: in Luau the _G table itself is a
  -- FROZEN snapshot (G[k] = v errors "readonly table"); getfenv(0) is
  -- the live environment in BOTH dialects (in 5.1 it is _G itself)
  local G = getfenv(0)
  local U = unpack
  local FLOOR = math.floor
  -- dialect probe: 5.1 tables have NO __len metamethod (5.2+/Luau do);
  -- the same VM source must behave per-host for the length operator
  local _probe = setmetatable({{}}, {{ __len = function() return 99 end }})
  local HAS_LEN_META = (_probe == nil) or false
  do
    local okp, vp = pcall(function() return #_probe end)
    HAS_LEN_META = okp and vp == 99
  end
  local function mget(x, k)
    local mt = getmetatable(x)
    if mt then return mt[k] end
    return nil
  end
  local function resolve_call(f)
    if type(f) == 'function' then return f, false end
    local mt = getmetatable(f)
    local cc = mt and mt['__call']
    if type(cc) == 'function' then return cc, true end
    if type(cc) == 'table' then
      local cf = cc[f]
      if type(cf) == 'function' then return cf, false end
    end
    error('attempt to call a ' .. type(f) .. ' value', 0)
  end
  -- call f(args) ONCE, return (values-table, exact count). A bare
  -- results-table loses trailing nils (and # with them), so the count
  -- comes from select on the same vararg list.
  local function callcap(f, args, nargs)
    local w = function(...)
      local t = {{ ... }}
      return t, select('#', ...)
    end
    return w(f(U(args, 1, nargs)))
  end
  local run
  local function makefn(idx, V, upsf)
    local pf = PF[idx]
    -- named `c` on purpose: the CREATING frame's cell array arrives as
    -- `upsf` (run()'s local) — needed for upvalue-alias binding
    local c = {{}}
    for i = 1, #pf.upsrc do
      local src = pf.upsrc[i]
      if src >= 49152 then
        -- upvalue alias: this frame itself materializes the symbol as
        -- upvalue src - 49152 — alias its CANONICAL cell object so all
        -- nesting levels share one cell (5.1 single-cell semantics)
        c[i] = upsf[src - 49152]
      elseif src >= 32768 then
        -- per-iteration shared cell: V[src - 32768] holds the CURRENT
        -- iteration's cell table [1] = value; every closure created
        -- in the same iteration binds the SAME table, and the loop
        -- body's own reads/writes go through it as well (fresh per
        -- iteration, shared within one iteration — 5.1 + Luau)
        c[i] = {{ v = V[src - 32768], i = 1 }}
      else
        c[i] = {{ v = V, i = src }}
      end
    end
    return function(...)
      local all = {{ ... }}
      local vargc = #all - pf.nparams
      if vargc < 0 then vargc = 0 end
      local vargs = {{}}
      for i = 1, vargc do vargs[i] = all[pf.nparams + i] end
      local V2 = {{}}
      for i = 1, pf.nparams do V2[i] = all[i] end
      local out, n = run(pf, V2, c, vargs, vargc)
      return U(out, 1, n)
    end
  end
  run = function(pf, V, ups, vargs, vargc)
    local B = pf.B
    local C = pf.C
    local pc = 1
    local lastn = 0
    local lastbase = 0
    while true do
      local oc = string.byte(B, pc); pc = pc + 1
      {fetch}
      {branches}
    end
  end
  local vargs = {{}}
  local V2 = {{}}
  local out, n = run(PF[#FN], V2, {{}}, vargs, 0)
  return U(out, 1, n)
end
"#,
		params = params,
		oc_table = oc_table,
		branches = branches,
		fetch = fetch,
	)
}
