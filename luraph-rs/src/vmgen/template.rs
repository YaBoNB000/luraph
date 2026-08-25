//! L6 VM — interpreter template generation.
//!
//! Emits the interpreter as Lua source (dialect-neutral, 5.1+Luau
//! compatible: no bitops, no `//`, no goto), then the project's own
//! parser turns it into an AST that runs through the full obfuscation
//! pipeline (mangle/flatten/strings/numbers/body/antidbg). Per build:
//! opcode codes are a random permutation (shared with the compiler)
//! and the dispatch branch order is shuffled.
//!
//! M5 surfaces encoded here:
//! - SoA parallel arrays (OC / SA / SB / SC / SD) after parse
//! - 7/14/21-bit varint decoder (r16) + 2^32 fold
//! - base-94 carrier + reserved-prefix token unescape
//! - decode-hub / state-tuple order randomized per build
//! - frame-runner primitive unpack from numbered slots

use crate::rng::Rng;
use crate::vmgen::isa::{Carrier, OpMap, CARRIER_SPECIALS, N_OPS};

/// Opcode names in base order (must match isa::op_index).
const OP_NAMES: [&str; N_OPS] = [
	"Jmp", "Jf", "Jt", "LoadNil", "LoadK", "Move", "Add", "Sub", "Mul", "Div",
	"Mod", "Pow", "Concat", "Unm", "Not", "Len", "Lt", "Le", "Gt", "Ge", "Eq",
	"Ne", "Idiv", "NewTab", "GetTab", "SetTab", "TabN", "CallT", "Closure",
	"Call", "VarArgTab", "VarArgC", "VarArgTabN", "GetGlobal", "SetGlobal",
	"GetUp", "SetUp", "Return", "Nop", "CallE", "CallM",
];

const PRIM_SRC: [&str; 15] = [
	"string.byte",
	"string.sub",
	"string.char",
	"getfenv",
	"unpack",
	"math.floor",
	"type",
	"error",
	"getmetatable",
	"rawget",
	"rawset",
	"setmetatable",
	"select",
	"pcall",
	"tonumber",
];

const PRIM_NAME: [&str; 15] = [
	"BYTE", "SUB", "CHAR", "GFE", "UNP", "FLR", "TYP", "ERR", "GMT", "RGET",
	"RSET", "SMT", "SEL", "PCAL", "TONUM",
];

/// The dispatch body of each opcode (referencing locals: oc, a, b, c, d,
/// pc, V, C, lastn, vargs, vargc, plus helpers mget/makefn and locals
/// G/U/FLOOR / TYP/ERR/GMT/RGET/RSET).
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
		"Concat" => "local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if (tx == 'number' or tx == 'string') and (ty == 'number' or ty == 'string') then V[a + 1] = x .. y else local f = mget(x, '__concat') or mget(y, '__concat'); if f then V[a + 1] = f(x, y) else ERR('attempt to perform concatenation on a ' .. tx .. ' value', 0) end end".to_string(),
		"Unm" => "local x = V[b + 1]; if TYP(x) == 'number' then V[a + 1] = -x else local f = mget(x, '__unm'); if f then V[a + 1] = f(x) else ERR('attempt to perform arithmetic on a ' .. TYP(x) .. ' value', 0) end end".to_string(),
		"Not" => "V[a + 1] = not V[b + 1]".to_string(),
		"Len" => "local x = V[b + 1]; local f = HAS_LEN_META and mget(x, '__len'); if f then V[a + 1] = f(x) else V[a + 1] = #x end".to_string(),
		"Lt" => cmp_code("x < y", "__lt", false),
		"Le" => cmp_code("x <= y", "__le", false),
		"Gt" => cmp_code("x > y", "__lt", true),
		"Ge" => cmp_code("x >= y", "__le", true),
		"Eq" => "local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if tx == ty and (tx == 'number' or tx == 'string' or tx == 'boolean' or tx == 'nil') then V[a + 1] = x == y else local f = mget(x, '__eq') or mget(y, '__eq'); if f then V[a + 1] = f(x, y) else V[a + 1] = x == y end end".to_string(),
		"Ne" => "local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); local eqv; if tx == ty and (tx == 'number' or tx == 'string' or tx == 'boolean' or tx == 'nil') then eqv = x == y else local f = mget(x, '__eq') or mget(y, '__eq'); if f then eqv = f(x, y) else eqv = x == y end end; V[a + 1] = not eqv".to_string(),
		"Idiv" => "V[a + 1] = FLOOR(V[b + 1] / V[c + 1])".to_string(),
		"NewTab" => "V[a + 1] = {}".to_string(),
		"GetTab" => "local t = V[b + 1]; local k = V[c + 1]; local r; if TYP(t) == 'table' then r = RGET(t, k); if r == nil then local f = mget(t, '__index'); if TYP(f) == 'function' then r = f(t, k) elseif f ~= nil then r = f[k] end end else local mt = GMT(t); if mt and mt['__index'] ~= nil then local f = mt['__index']; if TYP(f) == 'function' then r = f(t, k) else r = f[k] end else ERR('attempt to index a ' .. TYP(t) .. ' value', 0) end end; V[a + 1] = r".to_string(),
		"SetTab" => "local t = V[a + 1]; local k = V[b + 1]; local v = V[c + 1]; if TYP(t) ~= 'table' then local mt = GMT(t); local f = mt and mt['__newindex']; if TYP(f) == 'function' then f(t, k, v) elseif f ~= nil then RSET(f, k, v) else ERR('attempt to index a ' .. TYP(t) .. ' value', 0) end else if RGET(t, k) == nil then local f = mget(t, '__newindex'); if TYP(f) == 'function' then f(t, k, v) elseif f ~= nil then RSET(f, k, v) else RSET(t, k, v) end else RSET(t, k, v) end end".to_string(),
		"TabN" => "local t = V[a + 1]; local n = V[b + 1]; t[n + 1] = V[c + 1]; V[b + 1] = n + 1".to_string(),
		"CallT" => "local f = V[a + 1]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = FLOOR(d / 2); local tail = d % 2 == 1; local ntail = tail and V[a + nfixed + 2] or 0; local nargs = nfixed + off + ntail; local args = {}; if off == 1 then args[1] = f end; for i = 1, nfixed + ntail do if tail and i > nfixed then args[off + i] = V[a + i + 2] else args[off + i] = V[a + i + 1] end end; local t = V[b + 1]; local n = V[c + 1]; local out, nout = callcap(fn, args, nargs); for i = 1, nout do t[n + i] = out[i] end; V[c + 1] = n + nout".to_string(),
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
		"local x = V[b + 1]; local y = V[c + 1]; if TYP(x) == 'number' and TYP(y) == 'number' then V[a + 1] = {expr} else local f = mget(x, '{mm}') or mget(y, '{mm}'); if f then V[a + 1] = f(x, y) else ERR('attempt to perform {what} on a ' .. TYP(x) .. ' value', 0) end end"
	)
}

fn cmp_code(native: &str, mm: &str, swapped: bool) -> String {
	let call = if swapped { "f(y, x)" } else { "f(x, y)" };
	format!(
		"local x = V[b + 1]; local y = V[c + 1]; if TYP(x) == 'number' and TYP(y) == 'number' then V[a + 1] = {native} elseif TYP(x) == 'string' and TYP(y) == 'string' then V[a + 1] = {native} else local f = mget(x, '{mm}') or mget(y, '{mm}'); if f then V[a + 1] = {call} else ERR('attempt to compare ' .. TYP(x) .. ' with ' .. TYP(y), 0) end end"
	)
}

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

fn lua_dstr(bytes: &[u8]) -> String {
	let mut s = String::from("\"");
	for &b in bytes {
		s.push_str(&format!("\\{b:03}"));
	}
	s.push('"');
	s
}

/// Unique slots in 1..80 for the 15 primitives.
fn prim_slots(rng: &mut Rng) -> [u8; 15] {
	let mut pool: Vec<u8> = (1..=80).collect();
	rng.shuffle(&mut pool);
	let mut out = [0u8; 15];
	out.copy_from_slice(&pool[..15]);
	out
}

pub fn generate(
	map: &OpMap,
	slot_perm: &[u8; 4],
	carrier: &Carrier,
	rng: &mut Rng,
	n_fns: usize,
) -> String {
	let mut oc_items = Vec::new();
	for (i, name) in OP_NAMES.iter().enumerate() {
		oc_items.push(format!("{name} = {}", map.to_wire[i]));
	}
	let oc_table = format!("local OC = {{{}}}", oc_items.join(", "));

	let nop = nop_body(rng);
	let body_of = |name: &str| -> String {
		if name == "Nop" {
			nop.clone()
		} else {
			branch_code(name)
		}
	};
	let items: Vec<(usize, u8)> = (0..N_OPS).map(|i| (i, map.to_wire[i])).collect();
	let branches = gen_dispatch_tree(&items, &body_of, rng, 0);

	if std::env::var("LURAPH_VM_DBG").is_ok() {
		eprintln!("[gen] slot_perm={:?} reserved={}", slot_perm, carrier.reserved);
	}
	let mut pos_of = [1u8; 4];
	for (sl, &op_idx) in slot_perm.iter().enumerate() {
		pos_of[op_idx as usize] = sl as u8 + 1;
	}

	// primitive table: numbered slots + shuffled unpack
	let slots = prim_slots(rng);
	let mut bind_idx: Vec<usize> = (0..PRIM_SRC.len()).collect();
	rng.shuffle(&mut bind_idx);
	let mut p_fill = String::from("local P = {}\n");
	for &i in &bind_idx {
		p_fill.push_str(&format!("  P[{}] = {}\n", slots[i], PRIM_SRC[i]));
	}
	let unpack_lhs: Vec<&str> = bind_idx.iter().map(|&i| PRIM_NAME[i]).collect();
	let unpack_rhs: Vec<String> = bind_idx
		.iter()
		.map(|&i| format!("P[{}]", slots[i]))
		.collect();
	let prim_unpack = format!(
		"local {} = {}",
		unpack_lhs.join(", "),
		unpack_rhs.join(", ")
	);

	// frame-runner re-unpack (same slots, independently shuffled order)
	let mut run_idx: Vec<usize> = (0..PRIM_SRC.len()).collect();
	rng.shuffle(&mut run_idx);
	let run_lhs: Vec<&str> = run_idx.iter().map(|&i| PRIM_NAME[i]).collect();
	let run_rhs: Vec<String> = run_idx
		.iter()
		.map(|&i| format!("P[{}]", slots[i]))
		.collect();
	let run_unpack = format!(
		"local {} = {}",
		run_lhs.join(", "),
		run_rhs.join(", ")
	);

	// carrier decoder tables
	let mut al_lines = String::from("local AL = {}\n");
	for (i, &ch) in carrier.alphabet.iter().enumerate() {
		al_lines.push_str(&format!("  AL[{ch}] = {i}\n"));
	}
	let mut tk_lines = String::from("local TK = {}\n");
	for i in 0..10 {
		tk_lines.push_str(&format!(
			"  TK[{}] = {}\n",
			lua_dstr(carrier.tokens[i].as_bytes()),
			lua_dstr(&[CARRIER_SPECIALS[i]])
		));
	}

	// decode-hub / fetch: two styles, return-tuple order shuffled
	let use_hub = rng.int(0, 1) == 1;
	// 6-tuple identity: oc, t1, t2, t3, t4, pc'
	let mut tup: Vec<usize> = (0..6).collect();
	rng.shuffle(&mut tup);
	let srcs = [
		"W[pc]".to_string(),
		"SA[pc]".to_string(),
		"SB[pc]".to_string(),
		"SC[pc]".to_string(),
		"SD[pc]".to_string(),
		"pc + 1".to_string(),
	];
	let names = ["h0", "h1", "h2", "h3", "h4", "h5"];
	// inverse: which hub-return slot holds logical k
	let mut inv = [0usize; 6];
	for (slot, &logical) in tup.iter().enumerate() {
		inv[logical] = slot;
	}
	let fetch_assign = format!(
		"local oc = {n0}; local t1 = {n1}; local t2 = {n2}; local t3 = {n3}; local t4 = {n4}; pc = {n5}\n      local a = t{pa}; local b = t{pb}; local c = t{pc_}; local d = t{pd}",
		n0 = names[inv[0]],
		n1 = names[inv[1]],
		n2 = names[inv[2]],
		n3 = names[inv[3]],
		n4 = names[inv[4]],
		n5 = names[inv[5]],
		pa = pos_of[0],
		pb = pos_of[1],
		pc_ = pos_of[2],
		pd = pos_of[3],
	);
	let hub_ret: Vec<String> = tup.iter().map(|&i| srcs[i].clone()).collect();
	let hub_fn = format!(
		"local function hub(W, SA, SB, SC, SD, pc)\n    return {}\n  end",
		hub_ret.join(", ")
	);
	let (hub_decl, fetch) = if use_hub {
		(
			hub_fn,
			format!(
				"local {}, {}, {}, {}, {}, {} = hub(W, SA, SB, SC, SD, pc)\n      {fetch_assign}",
				names[0], names[1], names[2], names[3], names[4], names[5]
			),
		)
	} else {
		let binds: Vec<String> = (0..6)
			.map(|slot| format!("local {} = {}", names[slot], srcs[tup[slot]]))
			.collect();
		(
			"-- inline decode hub".to_string(),
			format!("{}\n      {fetch_assign}", binds.join("\n      ")),
		)
	};

	// run() SoA unpack order (state-tuple position)
	let mut fields = vec!["W", "SA", "SB", "SC", "SD", "C"];
	rng.shuffle(&mut fields);
	let run_soa = format!(
		"local {} = {}",
		fields.join(", "),
		fields
			.iter()
			.map(|f| format!("pf.{f}"))
			.collect::<Vec<_>>()
			.join(", ")
	);

	let mut params = Vec::new();
	for i in 1..=n_fns {
		params.push(format!("F{i}"));
	}
	let params = params.join(", ");

	// helper-decl order: u16 / r16 / decarrier are independent
	let u16_fn = r#"local function u16(B, p)
    return BYTE(B, p) + BYTE(B, p + 1) * 256
  end"#;
	let r16_fn = r#"local function r16(B, p)
    local b1 = BYTE(B, p)
    if b1 < 128 then
      return b1, p + 1
    end
    local b2 = BYTE(B, p + 1)
    if b2 < 128 then
      return (b1 - 128) + b2 * 128, p + 2
    end
    local b3 = BYTE(B, p + 2)
    if b3 < 128 then
      local v = (b1 - 128) + (b2 - 128) * 128 + b3 * 16384
      if v >= 2147483648 then v = v - 4294967296 end
      return v, p + 3
    end
    local b4 = BYTE(B, p + 3)
    local v = (b1 - 128) + (b2 - 128) * 128 + (b3 - 128) * 16384 + b4 * 2097152
    if v >= 2147483648 then v = v - 4294967296 end
    return v, p + 4
  end"#;
	let decarrier_fn = format!(
		r#"local function decarrier(s)
    local acc = ""
    local i = 1
    local n = #s
    local resv = {}
    while i <= n do
      if BYTE(s, i) == resv then
        acc = acc .. (TK[SUB(s, i, i + 4)] or "")
        i = i + 5
      else
        acc = acc .. SUB(s, i, i)
        i = i + 1
      end
    end
    local raw = ""
    n = #acc
    for i = 1, n, 5 do
      local v = 0
      v = v * 94 + AL[BYTE(acc, i)]
      v = v * 94 + AL[BYTE(acc, i + 1)]
      v = v * 94 + AL[BYTE(acc, i + 2)]
      v = v * 94 + AL[BYTE(acc, i + 3)]
      v = v * 94 + AL[BYTE(acc, i + 4)]
      local b1 = v % 256; v = FLR(v / 256)
      local b2 = v % 256; v = FLR(v / 256)
      local b3 = v % 256; v = FLR(v / 256)
      local b4 = v % 256
      raw = raw .. CHAR(b1, b2, b3, b4)
    end
    local ln = BYTE(raw, 1) + BYTE(raw, 2) * 256 + BYTE(raw, 3) * 65536 + BYTE(raw, 4) * 16777216
    return SUB(raw, 5, 4 + ln)
  end"#,
		carrier.reserved
	);

	// AL/TK must be declared BEFORE decarrier (Lua locals are visible
	// only after their declaration line; shuffle would turn them into
	// accidental globals).
	let mut helpers = vec![u16_fn.to_string(), r16_fn.to_string(), decarrier_fn];
	rng.shuffle(&mut helpers);
	let helpers = format!("{}\n  {}\n  {}", al_lines, tk_lines, helpers.join("\n  "));

	format!(
		r#"local VM = function({params})
  {oc_table}
  local FN = {{{params}}}
  local PF = {{}}
  {p_fill}  {prim_unpack}
  {helpers}
  local function parse(s)
    s = decarrier(s)
    local p = 1
    local nregs = u16(s, p); p = p + 2
    local nparams = u16(s, p); p = p + 2
    local vararg = BYTE(s, p); p = p + 1
    local nups = u16(s, p); p = p + 2
    local upsrc = {{}}
    for i = 1, nups do upsrc[i] = u16(s, p); p = p + 2 end
    local nconst = u16(s, p); p = p + 2
    local C = {{}}
    for i = 1, nconst do
      local t = BYTE(s, p); p = p + 1
      if t == 0 then
        C[i] = nil
      elseif t == 1 then
        C[i] = BYTE(s, p) == 1; p = p + 1
      else
        local l = u16(s, p); p = p + 2
        local x = SUB(s, p, p + l - 1); p = p + l
        if t == 2 then C[i] = TONUM(x) else C[i] = x end
      end
    end
    local ncode = u16(s, p); p = p + 2
    local W = {{}}
    for i = 1, ncode do W[i] = BYTE(s, p); p = p + 1 end
    local function rstream()
      local T = {{}}
      for i = 1, ncode do
        local v, np = r16(s, p); p = np; T[i] = v
      end
      return T
    end
    local SA, SB, SC, SD = rstream(), rstream(), rstream(), rstream()
    return {{ nregs = nregs, nparams = nparams, vararg = vararg, upsrc = upsrc, C = C, W = W, SA = SA, SB = SB, SC = SC, SD = SD }}
  end
  for i = 1, #FN do PF[i] = parse(FN[i]) end
  local G = GFE(0)
  local U = UNP
  local FLOOR = FLR
  local _probe = SMT({{}}, {{ __len = function() return 99 end }})
  local HAS_LEN_META = (_probe == nil) or false
  do
    local okp, vp = PCAL(function() return #_probe end)
    HAS_LEN_META = okp and vp == 99
  end
  local function mget(x, k)
    local mt = GMT(x)
    if mt then return mt[k] end
    return nil
  end
  local function resolve_call(f)
    if TYP(f) == 'function' then return f, false end
    local mt = GMT(f)
    local cc = mt and mt['__call']
    if TYP(cc) == 'function' then return cc, true end
    if TYP(cc) == 'table' then
      local cf = cc[f]
      if TYP(cf) == 'function' then return cf, false end
    end
    ERR('attempt to call a ' .. TYP(f) .. ' value', 0)
  end
  local function callcap(f, args, nargs)
    local w = function(...)
      local t = {{ ... }}
      return t, SEL('#', ...)
    end
    return w(f(U(args, 1, nargs)))
  end
  {hub_decl}
  local run
  local function makefn(idx, V, upsf)
    local pf = PF[idx]
    local c = {{}}
    for i = 1, #pf.upsrc do
      local src = pf.upsrc[i]
      if src >= 49152 then
        c[i] = upsf[src - 49152]
      elseif src >= 32768 then
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
    {run_unpack}
    {run_soa}
    local pc = 1
    local lastn = 0
    local lastbase = 0
    while true do
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
		p_fill = p_fill,
		prim_unpack = prim_unpack,
		helpers = helpers,
		hub_decl = hub_decl,
		run_unpack = run_unpack,
		run_soa = run_soa,
		fetch = fetch,
		branches = branches,
	)
}
