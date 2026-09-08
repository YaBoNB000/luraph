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
//!
//! 建议1: every opcode's interpreter body lives in its own file under
//! `vmgen/handlers/`; this assembler asks each file for its fixed code
//! (one FORMAT per instruction per build, chosen at random), shuffles
//! the dispatch leaf order and the handler-definition order on every
//! generation, and the wire codes are a per-build permutation (OpMap).

use crate::rng::Rng;
use crate::vmgen::handlers;
use crate::vmgen::isa::{fold_key, Carrier, OpMap, CARRIER_SPECIALS, N_OPS};
use crate::vmgen::strpool::StrPool;

/// Opcode names in base order (must match isa::op_index).
const OP_NAMES: [&str; N_OPS] = [
	"Jmp", "Jf", "Jt", "LoadNil", "LoadK", "Move", "Add", "Sub", "Mul", "Div",
	"Mod", "Pow", "Concat", "Unm", "Not", "Len", "Lt", "Le", "Gt", "Ge", "Eq",
	"Ne", "Idiv", "NewTab", "GetTab", "SetTab", "TabN", "CallT", "Closure",
	"Call", "VarArgTab", "VarArgC", "VarArgTabN", "GetGlobal", "SetGlobal",
	"GetUp", "SetUp", "Return", "Nop", "CallE", "CallM", "MkStr",
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

/// M5 random decision-tree dispatch. Threshold splits on the wire codes
/// with a random pivot + random comparison form; leaves are equality
/// tests; at the depth cap a flat if/elseif bottom is emitted, so the
/// visible nesting stays in the 2~4 layer band. Per-build shape.
fn gen_dispatch_tree(
	items: &[(String, u8)],
	body_of: &mut dyn FnMut(&str) -> String,
	rng: &mut Rng,
	depth: u32,
) -> String {
	let n = items.len();
	if n == 1 {
		let name = items[0].0.as_str();
		return format!("if oc == OC.{} then\n{}\nend", name, body_of(name));
	}
	if depth >= 3 {
		let mut v: Vec<(String, u8)> = items.to_vec();
		rng.shuffle(&mut v);
		let mut s = String::new();
		for (i, (idx, _)) in v.iter().enumerate() {
			let name = idx.as_str();
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
		let (left, right): (Vec<(String, u8)>, Vec<(String, u8)>) =
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
		let (left, right): (Vec<(String, u8)>, Vec<(String, u8)>) =
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

/// 增量⑪ (防静态, 报告突破口 #6/#10): obfuscate an integer so it no
/// longer appears as a bare literal (the attacker used the hardcoded
/// checksums as oracles to verify their decryption). Returns a Lua
/// expression EXACTLY equal to `n` (doubles are exact for |n|<2^53).
/// Decomposes into `x + y - z` with random x,y (z = x+y-n) so there is
/// no trivially-simplifiable `a+(n-a)` shape.
fn obf_num(n: u64, rng: &mut Rng) -> String {
	// keep x,y below 2^40 so x+y-n stays exact in a double
	let x = rng.int(0, 1_099_511_627_775) as i64; // < 2^40
	let y = rng.int(0, 1_099_511_627_775) as i64;
	let z = x + y - n as i64;
	format!("({} + {} - {})", x, y, z)
}

use super::manifest_key;

/// 增量⑳ (选项B·激活值多因子化): nonlinear activation hash shared by
/// the compile-time AK fold (Rust) and the runtime gate (Lua/bit32).
/// Must stay byte-for-byte identical to the Lua mirror in the boot gate:
///   h ^= b; h = lrotate(h, rot); h ^= (h*mul + b) & 0xFFFFFFFF
/// The xor/rotate/multiply mix is nonlinear (fold31 was linear and
/// algebraically foldable), and two independent (seed,rot,mul) factors
/// are mixed so a collision must satisfy both at once.
fn activation_hash(key: &[u8], seed: u32, rot: u32, mul: u32) -> u32 {
	let mut h: u32 = seed;
	for &b in key {
		h ^= b as u32;
		h = h.rotate_left(rot);
		h ^= h.wrapping_mul(mul).wrapping_add(b as u32);
	}
	h
}

/// 增量⑩ (防静态, 报告突破口 #5/#2) + 增量⑬ (对抗 R002 符号求值):
/// key material is never emitted as a literal AND no key has a closed
/// arithmetic form. R002 folded every `(A*B+C)%2^28` fragment recipe
/// offline, so increment ⑬ moves assembly through per-build random
/// LOOKUP TABLES: each key is a sum/product of entries from four baked
/// tables (KA/KB/KC random 28-bit values, KM small multipliers). The
/// tables sit in the output as plain number arrays; recovering a key
/// now requires data-flow tracing from the use site through obfuscated
/// indexes into the right tables — no pattern to recognize, no formula
/// to fold. Recipes (drawn per key):
///
///   add2     `(KA[a] + KB[b]) % 2^28`
///   add3     `(KA[a] + KB[b] + KC[c]) % 2^28`
///   affine   `(KA[a] * KM[m] + KC[c]) % 2^28`   (product < 2^45, exact)
///   anchored `(KA[a] + KM[m] * <anchor>) % 2^28` (anchor = a boot-time
///             table length, e.g. #APH / #hqi)
///
/// Indexes go through obf_num as before. All arithmetic stays exact in
/// doubles.
const KEY_MOD: i64 = 268_435_456; // 2^28
const KT_ENTRIES: usize = 64;

struct KeyEmitter {
	/// table values; None = still free (filled randomly at emission)
	ka: Vec<Option<i64>>,
	kb: Vec<Option<i64>>,
	kc: Vec<Option<i64>>,
	km: Vec<Option<i64>>,
	/// shuffled free 1-based positions per table
	pa: Vec<usize>,
	pb: Vec<usize>,
	pc: Vec<usize>,
	pm: Vec<usize>,
}

impl KeyEmitter {
	fn new(rng: &mut Rng) -> KeyEmitter {
		let mut mk = || {
			let mut p: Vec<usize> = (1..=KT_ENTRIES).collect();
			rng.shuffle(&mut p);
			p
		};
		KeyEmitter {
			ka: vec![None; KT_ENTRIES],
			kb: vec![None; KT_ENTRIES],
			kc: vec![None; KT_ENTRIES],
			km: vec![None; KT_ENTRIES],
			pa: mk(),
			pb: mk(),
			pc: mk(),
			pm: mk(),
		}
	}
	/// Reserve one entry in `tab` and store `v`; returns its 1-based
	/// position (the recipe re-obfuscates the index on emission).
	fn put(&mut self, tab: u8, v: i64) -> usize {
		let (vals, poss) = match tab {
			0 => (&mut self.ka, &mut self.pa),
			1 => (&mut self.kb, &mut self.pb),
			2 => (&mut self.kc, &mut self.pc),
			_ => (&mut self.km, &mut self.pm),
		};
		let pos = poss.pop().expect("key-table exhaustion");
		vals[pos - 1] = Some(v);
		pos
	}
	/// Assembly expression that evaluates to `key` at runtime.
	/// `anchor` = (Lua expression, its build-known runtime value).
	fn key_expr(
		&mut self,
		key: i64,
		anchor: Option<(&str, i64)>,
		rng: &mut Rng,
	) -> String {
		if std::env::var("LURAPH_KEY_DBG").is_ok() {
			return format!("{}", key);
		}
		let idx = |v: usize, rng: &mut Rng| obf_num(v as u64, rng);
		let form = if anchor.is_some() { rng.int(0, 3) } else { rng.int(0, 2) };
		match form {
			0 => {
				// add2: K = (A + B) % M
				let a = rng.int(1_048_576, KEY_MOD - 1);
				let b = (key - a).rem_euclid(KEY_MOD);
				let ia = self.put(0, a);
				let ib = self.put(1, b);
				format!(
					"((KA[{}] + KB[{}]) % {})",
					idx(ia, rng),
					idx(ib, rng),
					KEY_MOD
				)
			}
			1 => {
				// add3: K = (A + B + C) % M
				let a = rng.int(1_048_576, KEY_MOD - 1);
				let b = rng.int(1_048_576, KEY_MOD - 1);
				let c = (key - a - b).rem_euclid(KEY_MOD);
				let ia = self.put(0, a);
				let ib = self.put(1, b);
				let ic = self.put(2, c);
				format!(
					"((KA[{}] + KB[{}] + KC[{}]) % {})",
					idx(ia, rng),
					idx(ib, rng),
					idx(ic, rng),
					KEY_MOD
				)
			}
			2 => {
				// affine: K = (A*m + C) % M (m small, product exact)
				let a = rng.int(1_048_576, KEY_MOD - 1);
				let m = rng.int(1, 16383) | 1;
				let c = (key - a * m).rem_euclid(KEY_MOD);
				let ia = self.put(0, a);
				let im = self.put(3, m);
				let ic = self.put(2, c);
				format!(
					"((KA[{}] * KM[{}] + KC[{}]) % {})",
					idx(ia, rng),
					idx(im, rng),
					idx(ic, rng),
					KEY_MOD
				)
			}
			_ => {
				// anchored: K = (A + m*anchor) % M
				let (ae, av) = anchor.unwrap();
				let m = rng.int(1, 999);
				let a = (key - m * av).rem_euclid(KEY_MOD);
				let ia = self.put(0, a);
				let im = self.put(3, m);
				format!(
					"((KA[{}] + KM[{}] * {}) % {})",
					idx(ia, rng),
					idx(im, rng),
					ae,
					KEY_MOD
				)
			}
		}
	}
	/// 增量⑬ (R002 §3.37 — dead-stage proof): an expression that is 0
	/// at runtime but PROVABLY so only by fetching three table values:
	/// the reserved KA/KB/KC entries sum to 0 mod 7 by construction.
	fn dead_zero(&mut self, rng: &mut Rng) -> String {
		let a = rng.int(1_048_576, KEY_MOD - 1);
		let b = rng.int(1_048_576, KEY_MOD - 1);
		let base = rng.int(1_048_576, KEY_MOD - 1);
		let fix = (7 - (a + b + base).rem_euclid(7)) % 7;
		let c = base + fix;
		let ia = self.put(0, a);
		let ib = self.put(1, b);
		let ic = self.put(2, c);
		format!(
			"((KA[{}] + KB[{}] + KC[{}]) % 7)",
			obf_num(ia as u64, rng),
			obf_num(ib as u64, rng),
			obf_num(ic as u64, rng)
		)
	}
	/// Emit the four lookup tables (free entries filled with random
	/// camouflage values).
	fn block(mut self, rng: &mut Rng) -> String {
		let fill = |vals: &mut Vec<Option<i64>>, lo: i64, hi: i64, rng: &mut Rng| {
			for v in vals.iter_mut() {
				if v.is_none() {
					*v = Some(rng.int(lo, hi));
				}
			}
		};
		fill(&mut self.ka, 1_048_576, KEY_MOD - 1, rng);
		fill(&mut self.kb, 1_048_576, KEY_MOD - 1, rng);
		fill(&mut self.kc, 1_048_576, KEY_MOD - 1, rng);
		fill(&mut self.km, 1, 16383, rng);
		let emit = |name: &str, vals: &Vec<Option<i64>>| {
			let body = vals
				.iter()
				.map(|v| v.unwrap().to_string())
				.collect::<Vec<_>>()
				.join(", ");
			format!("local {} = {{{}}}", name, body)
		};
		format!(
			"{}\n  {}\n  {}\n  {}\n  ",
			emit("KA", &self.ka),
			emit("KB", &self.kb),
			emit("KC", &self.kc),
			emit("KM", &self.km)
		)
	}
}

/// 增量⑰-A (对抗 R003 — 引导迷你 VM): the meta keystream generation +
/// HBOOT unmask loop is emitted as a small custom bytecode program run
/// by a visible dispatcher. R003's whitelist folder handles arithmetic
/// expressions but NOT opcode-dispatch semantics: recovering this layer
/// requires ISA extraction + emulation with indirect memory access,
/// self-modification and data-dependent branches (their stated 3-6x
/// band). Per build: opcode numbering permuted, memory bases randomized.
///
/// ISA (2-operand, registers R[0..7], memory MM):
///   LDI d,x : R[d] = MP[x]       LDM d,a: R[d] = MM[MP[a]]
///   STM a,s : MM[MP[a]] = R[s]   LD d,s : R[d] = MM[R[s]]
///   ST s,d  : MM[R[s]] = R[d]    MOV d,s: R[d] = R[s]
///   ADD/SUB/MUL d,s              DIV d,s: R[d]=FLR(R[d]/R[s])
///   MOD d,s : R[d] = R[d]%R[s]   (Lua %, non-negative result)
///   JZ s,off: if R[s]==0 pc+=off else pc+=3
///   JMP off : pc += off          STP x,s: MP[x] = R[s]
///   HALT
/// Program computes: for i=1..n: state=(m*state+c)%2^28;
/// out[i] = (in[i] - fold4(state)) % 256. The fold divisor 256 is
/// computed at RUNTIME (repeated doubling) and SELF-MODIFIED into the
/// program array (STP) before first use.
fn emit_metavm(seed_e: &str, m_e: &str, c_e: &str, mb_count: usize, rng: &mut Rng) -> String {
	// randomized opcode numbering (1..=15 permuted)
	let names = [
		"LDI", "LDM", "STM", "LD", "ST", "MOV", "ADD", "SUB", "MUL", "DIV",
		"MOD", "JZ", "JMP", "STP", "HALT",
	];
	let mut nums: Vec<i64> = (1..=15).collect();
	rng.shuffle(&mut nums);
	let n: std::collections::HashMap<&str, i64> = names
		.iter()
		.zip(nums.iter())
		.map(|(a, b)| (*a, *b))
		.collect();
	let in_base: i64 = 100 + rng.int(0, 400);
	let out_base: i64 = in_base + mb_count as i64 + 50 + rng.int(0, 200);
	let scratch: i64 = out_base + mb_count as i64 + 20 + rng.int(0, 40);

	// ---- fixed program layout (word offsets; 0-based) -------------
	// regs: 0=state 1=m 2=c 3=i 4=n 5..7 temps
	let mut p: Vec<i64> = Vec::new();
	// helpers: absolute const positions resolved after code gen
	// code:
	// 0..26  load state/m/c/n from MM[1..4]; i=1
	// 27..53 r7 = 256 by doubling
	// 54     STP c256_slot, r7   (self-mod)
	// 57=loop: LCG step + fold4 + byte IO + i++ + branch
	//
	// word counts: LDI/LDM/STM/LD/ST/MOV/ADD/SUB/MUL/DIV/MOD/STP = 3,
	// JZ = 3, JMP = 2, HALT = 1.
	let o3 = |p: &mut Vec<i64>, op: i64, a: i64, b: i64| p.extend_from_slice(&[op, a, b]);
	// init: LDI r7, <1> ; LD r0, r7 ; ... addresses 1..4 as constants
	// (the constant value doubles as the MM address it names)
	// -- const slots appended later; use symbolic refs patched below
	const FIXUP: i64 = -1;
	let ldi = |p: &mut Vec<i64>, n: &std::collections::HashMap<&str, i64>, d: i64, sym: i64| {
		o3(p, n["LDI"], d, sym); // sym = FIXUP placeholder index marker
	};
	let _ = ldi;
	// Emit with raw markers: const refs encoded as -(marker) and patched
	// marker ids: 1=one 2=two 3=three 4=four 5=zero 6=mod28 7=ib 8=ob
	//             9=c256 10=sa
	macro_rules! ins {
		($op:expr, $a:expr, $b:expr) => {
			p.extend_from_slice(&[n[$op], $a, $b])
		};
	}
	macro_rules! ins1 {
		($op:expr, $a:expr) => {
			p.extend_from_slice(&[n[$op], $a])
		};
	}
	macro_rules! ins0 {
		($op:expr) => {
			p.push(n[$op])
		};
	}
	// markers as negative x-operand for LDI/STP; patched after layout
	// (positions are 1-based in the final array)
	let mk = |id: i64| -id; // symbolic
	// --- init ---
	ins!("LDI", 7, mk(1)); // r7 = 1
	ins!("LD", 0, 7);      // state = MM[1]
	ins!("LDI", 7, mk(2)); // r7 = 2
	ins!("LD", 1, 7);      // m = MM[2]
	ins!("LDI", 7, mk(3)); // r7 = 3
	ins!("LD", 2, 7);      // c = MM[3]
	ins!("LDI", 7, mk(4)); // r7 = 4
	ins!("LD", 4, 7);      // n = MM[4]
	ins!("LDI", 3, mk(1)); // i = 1
	ins!("LDI", 7, mk(1)); // r7 = 1 (doubling seed)
	for _ in 0..8 {
		ins!("ADD", 7, 7); // r7 *= 2
	}
	ins!("STP", mk(9), 7); // MP[c256] = 256  (self-modify)
	// --- loop ---
	let loop_pos = p.len() as i64;
	ins!("MUL", 0, 1);
	ins!("ADD", 0, 2);
	ins!("LDI", 7, mk(6)); // 2^28
	ins!("MOD", 0, 7);     // state %= 2^28
	// fold4(state) -> r5
	ins!("LDI", 7, mk(9)); // 256 (self-modified slot)
	ins!("MOV", 6, 0);     // t = state
	ins!("LDI", 5, mk(5)); // acc = 0
	for _ in 0..4 {
		ins!("STM", mk(10), 6); // MM[scratch] = t
		ins!("MOD", 6, 7);      // t % 256
		ins!("ADD", 5, 6);      // acc +=
		ins!("LDM", 6, mk(10)); // t = MM[scratch]
		ins!("DIV", 6, 7);      // t /= 256
	}
	ins!("MOD", 5, 7);     // key = acc % 256
	// byte IO: out[i] = (in[i] - key) % 256
	ins!("LDI", 6, mk(7)); // IB
	ins!("ADD", 6, 3);     // IB + i
	ins!("LDI", 7, mk(1)); // 1
	ins!("SUB", 6, 7);     // addr_in = IB + i - 1
	ins!("LD", 6, 6);      // in = MM[addr_in]
	ins!("SUB", 6, 5);     // in - key
	ins!("LDI", 7, mk(9)); // 256
	ins!("MOD", 6, 7);     // % 256 (Lua handles negatives)
	ins!("LDI", 7, mk(8)); // OB
	ins!("ADD", 7, 3);     // OB + i
	ins!("LDI", 5, mk(1)); // 1 (key consumed)
	ins!("SUB", 7, 5);     // addr_out = OB + i - 1
	ins!("ST", 7, 6);      // MM[addr_out] = byte
	ins!("ADD", 3, 5);     // i += 1
	// branch: continue while n - i + 1 != 0
	ins!("MOV", 6, 4);
	ins!("SUB", 6, 3);
	ins!("ADD", 6, 5);
	let jz_pos = p.len() as i64;
	ins!("JZ", 6, FIXUP); // offset patched: skip over JMP to HALT
	ins1!("JMP", FIXUP);  // offset patched: back to loop
	ins0!("HALT");
	let halt_pos = p.len() as i64 - 1;
	// ---- constants appended after code ------------------------------
	let mut const_pos: [i64; 11] = [0; 11];
	let vals: [i64; 10] = [
		1, 2, 3, 4, 0, 268435456, in_base, out_base, 0, scratch,
	];
	for (k, v) in vals.iter().enumerate() {
		const_pos[k + 1] = p.len() as i64 + 1; // 1-based position
		p.push(*v);
	}
	// patch symbolic markers (operands stored as -id) FIRST, then
	// JZ/JMP relative offsets (which may be negative words)
	for w in p.iter_mut() {
		if *w < 0 {
			let id = (-*w) as usize;
			*w = const_pos[id];
		}
	}
	p[jz_pos as usize + 2] = halt_pos - jz_pos; // JZ: land on HALT
	p[jz_pos as usize + 4] = loop_pos - (jz_pos + 3); // JMP: back to loop
	// ---- emit -------------------------------------------------------
	let mut mp = String::from("local MP = {");
	mp.push_str(
		&p.iter()
			.map(|v| v.to_string())
			.collect::<Vec<_>>()
			.join(", "),
	);
	mp.push_str("}\n");
	let gather = format!(
		"local MM = {{}}\n    MM[1] = {}\n    MM[2] = {}\n    MM[3] = {}\n    MM[4] = #MB\n    for i = 1, #MB do MM[{} + i] = MB[i] end\n",
		seed_e, m_e, c_e, in_base - 1
	);
	let mut disp = String::from(
		"    local MR = {0, 0, 0, 0, 0, 0, 0, 0}\n    local mpc = 1\n    while true do\n      local mop = MP[mpc]\n",
	);
	let d3 = format!(
		"      if mop == {} then local d = MP[mpc + 1]; MR[d] = MP[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["LDI"]
	);
	let ldm3 = format!(
		"      elseif mop == {} then local d = MP[mpc + 1]; MR[d] = MM[MP[MP[mpc + 2]]]; mpc = mpc + 3\n",
		n["LDM"]
	);
	let stm3 = format!(
		"      elseif mop == {} then MM[MP[MP[mpc + 1]]] = MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["STM"]
	);
	let ld3 = format!(
		"      elseif mop == {} then local d = MP[mpc + 1]; MR[d] = MM[MR[MP[mpc + 2]]]; mpc = mpc + 3\n",
		n["LD"]
	);
	let st3 = format!(
		"      elseif mop == {} then MM[MR[MP[mpc + 1]]] = MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["ST"]
	);
	let mov3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["MOV"]
	);
	let add3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = MR[MP[mpc + 1]] + MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["ADD"]
	);
	let sub3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = MR[MP[mpc + 1]] - MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["SUB"]
	);
	let mul3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = MR[MP[mpc + 1]] * MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["MUL"]
	);
	let div3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = FLR(MR[MP[mpc + 1]] / MR[MP[mpc + 2]]); mpc = mpc + 3\n",
		n["DIV"]
	);
	let mod3 = format!(
		"      elseif mop == {} then MR[MP[mpc + 1]] = MR[MP[mpc + 1]] % MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["MOD"]
	);
	let jz3 = format!(
		"      elseif mop == {} then if MR[MP[mpc + 1]] == 0 then mpc = mpc + MP[mpc + 2] else mpc = mpc + 3 end\n",
		n["JZ"]
	);
	let jmp2 = format!(
		"      elseif mop == {} then mpc = mpc + MP[mpc + 1]\n",
		n["JMP"]
	);
	let stp3 = format!(
		"      elseif mop == {} then MP[MP[mpc + 1]] = MR[MP[mpc + 2]]; mpc = mpc + 3\n",
		n["STP"]
	);
	disp.push_str(&d3);
	disp.push_str(&ldm3);
	disp.push_str(&stm3);
	disp.push_str(&ld3);
	disp.push_str(&st3);
	disp.push_str(&mov3);
	disp.push_str(&add3);
	disp.push_str(&sub3);
	disp.push_str(&mul3);
	disp.push_str(&div3);
	disp.push_str(&mod3);
	disp.push_str(&jz3);
	disp.push_str(&jmp2);
	disp.push_str(&stp3);
	disp.push_str("      else break end\n    end\n");
	let mh = format!(
		"    local MH = {{}}\n    for i = 1, MM[4] do MH[i] = CHAR(MM[{} + i]) end\n",
		out_base - 1
	);
	let out = format!("{}{}{}{}", mp, gather, disp, mh);
	if std::env::var("LURAPH_MVM_DBG").is_ok() {
		let map_txt: Vec<String> = names.iter().map(|k| format!("{}={}", k, n[k])).collect();
		eprintln!("MVMOPS {} INB {} OUTB {} SCR {}", map_txt.join(","), in_base, out_base, scratch);
		eprintln!("MVM>>>{}<<<MVM", out);
	}
	out
}

/// P4 (防御代码隐藏): runtime string-builder for the interpreter
/// scope — char codes stored SHUFFLED in a table plus an order list,
/// concatenated through CHAR. Returns Lua declarations; the built
/// value ends up in `var`.
fn coded_name_tpl(rng: &mut Rng, var: &str, name: &str) -> String {
	let codes: Vec<u8> = name.bytes().collect();
	let mut pos: Vec<usize> = (0..codes.len()).collect();
	rng.shuffle(&mut pos);
	let mut out = format!("local {var}t = {{}}\n");
	for (i, &c) in codes.iter().enumerate() {
		out.push_str(&format!("    {var}t[{}] = {c}\n", pos[i] + 1));
	}
	let mut inv = vec![0usize; codes.len()];
	for (i, &p) in pos.iter().enumerate() {
		inv[i] = p + 1;
	}
	out.push_str(&format!(
		"    local {var}o = {{{}}}\n    local {var} = \"\"\n    for {var}i = 1, #{var}o do\n      {var} = {var} .. CHAR({var}t[{var}o[{var}i]])\n    end\n",
		inv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
	));
	out
}

/// 增量⑲ (选项B路线二 — CPS 去中心化): one handler's fetch+dispatch
/// epilogue — a PROPER tail call into the next handler (pc threads
/// through E.pc). One of five semantics-identical shapes with fresh
/// per-epilogue temp names; every handler fragment carries its own
/// copy, so the dispatch logic is scattered across all 43 encrypted
/// bodies (no single hookable choke point, R006 barrier ②).
fn chain_epilogue(rng: &mut Rng, s0: &str, s1: &str, s2: &str, s3: &str) -> String {
	// temp names that never collide with handler-body locals
	// (_/b0/c0/cf/d0/eqv/f/fn/i/j/k/m0/ms/msk/mt/n/nargs/.../x/y) nor
	// with the env prelude (V/C/S/O/G/U/MS/W/SA../AV/HW2/CHAR/...)
	const NP_POOL: [&str; 6] = ["np", "gz", "hz", "kz", "pz", "wz"];
	const OC_POOL: [&str; 6] = ["oc", "jz", "qz", "xz", "yz", "nz"];
	const H_POOL: [&str; 6] = ["hx", "fx", "zx", "vx", "mx", "jx"];
	let np = NP_POOL[rng.int(0, 5) as usize];
	let oc = OC_POOL[rng.int(0, 5) as usize];
	let h = H_POOL[rng.int(0, 5) as usize];
	// 平台事实（实测+官方）：Luau 已删除正确尾调用（devforum 3307934:
	// "Tail calls were removed from Luau... no plans to revive"），调用
	// 链每步积 1 帧、~16k 帧溢栈。纯尾链跑不了任意循环——故每个分派
	// 点先消费预算 `E.b`：预算耗尽时空返回，整条链解开回落到蹦床循环
	// （深度恒 ≤ K，嵌套调用安全）；蹦床重置预算后从 E.pc 重入。K 步
	// 一次解链，额外开销 ~3%；重入点本体也是加密碎片（同形状），可见
	// 区只剩无操作码语义的蹦床（R006 壁垒② 的 Luau-可行形态）。
	let budget = if rng.int(0, 1) == 0 {
		"E.b = E.b - 1; if E.b == 0 then return end; "
	} else {
		"E.b = E.b - 1; if E.b <= 0 then return end; "
	};
	match rng.int(0, 4) {
		0 => format!(
			"{budget}local {np} = E.pc; local {oc} = W[{np}]; E.pc = {np} + 1; return HW2[({oc} + AV) % 256](E, {s0}[{np}], {s1}[{np}], {s2}[{np}], {s3}[{np}])",
			budget = budget, np = np, oc = oc, s0 = s0, s1 = s1, s2 = s2, s3 = s3
		),
		1 => format!(
			"{budget}local {np} = E.pc; E.pc = {np} + 1; local {oc} = W[{np}]; local {h} = HW2[({oc} + AV) % 256]; return {h}(E, {s0}[{np}], {s1}[{np}], {s2}[{np}], {s3}[{np}])",
			budget = budget, np = np, oc = oc, h = h, s0 = s0, s1 = s1, s2 = s2, s3 = s3
		),
		2 => format!(
			"{budget}local {np}, {oc} = E.pc, W[E.pc]; E.pc = {np} + 1; return HW2[({oc} + AV) % 256](E, {s0}[{np}], {s1}[{np}], {s2}[{np}], {s3}[{np}])",
			budget = budget, np = np, oc = oc, s0 = s0, s1 = s1, s2 = s2, s3 = s3
		),
		3 => format!(
			"{budget}local {np} = E.pc; local {h} = HW2[(W[{np}] + AV) % 256]; E.pc = {np} + 1; return {h}(E, {s0}[{np}], {s1}[{np}], {s2}[{np}], {s3}[{np}])",
			budget = budget, np = np, h = h, s0 = s0, s1 = s1, s2 = s2, s3 = s3
		),
		_ => format!(
			"{budget}local {np} = E.pc; local {oc} = W[{np}]; local oa, ob, od2, oe = {s0}[{np}], {s1}[{np}], {s2}[{np}], {s3}[{np}]; E.pc = {np} + 1; return HW2[({oc} + AV) % 256](E, oa, ob, od2, oe)",
			budget = budget, np = np, oc = oc, s0 = s0, s1 = s1, s2 = s2, s3 = s3
		),
	}
}

pub fn generate(
	map: &OpMap,
	slot_perm: &[u8; 4],
	carrier: &Carrier,
	rng: &mut Rng,
	n_fns: usize,
	v15: bool,
	nop_sites: &[Vec<u16>],
	operand_sums: &[u64],
	ck: (u32, u32),
	mk: (u16, u16),
	blobk: (u32, u32, u32, u32),
	tags: [u8; 7],
	// ㉒ (选项B — B-2 碎片即用即毁): numeric constants are exact-
	// additive-mask safe (see VmProgram::consts_mask_safe). v15 only.
	consts_safe: bool,
	// 增量⑱ (选项B路线一 — 输入绑定/激活门): activation string known
	// at obfuscation time. v15 only (asserted below).
	bind_key: Option<&str>,
) -> String {
	assert!(
		bind_key.is_none() || v15,
		"--bind-key rides the v15 boot chain (legacy VM has no gate)"
	);
	let mut oc_items = Vec::new();
	for (i, name) in OP_NAMES.iter().enumerate() {
		oc_items.push(format!("{name} = {}", map.to_wire[i]));
	}
	let oc_table = format!("local OC = {{{}}}", oc_items.join(", "));
	// P3b (自描述消除, v15): no named opcode table in the output.
	// The wire array is built at BOOT from shuffled, per-index masked
	// pairs (mask = (ocm + idx*occ) % 65536 over the wire byte); the
	// dispatch compares against OCt[<op_index>] positionally. The Nop
	// alias becomes NOPA = (p1 + p2) % 256 (no literal alias byte).
	// 增量⑩: key-fragment emitter — every key constant below is drawn
	// through ke.key_expr (no bare key literal survives in the output).
	let mut ke = KeyEmitter::new(rng);
	let oc_boot: String = if v15 {
		let ocm = rng.int(1, 65535);
		let occ = rng.int(1, 65535);
		let mut pairs: Vec<String> = Vec::new();
		for i in 0..N_OPS {
			let mask = (ocm + i as i64 * occ) % 65536;
			let stored = (map.to_wire[i] as i64 + mask) % 256;
			pairs.push(format!("{}, {}", i, stored));
		}
		rng.shuffle(&mut pairs);
		let p1 = rng.int(0, 255);
		let p2 = ((map.nop_alias as i64 - p1) % 256 + 256) % 256;
		// 增量⑩: the wire-mask keys and the Nop-alias sum are assembled
		// from KF fragments; #ocp (built right here at boot) is the
		// runtime anchor (2*N_OPS entries).
		let ocm_e = ke.key_expr(ocm, Some(("#ocp", 2 * N_OPS as i64)), rng);
		let occ_e = ke.key_expr(occ, Some(("#ocp", 2 * N_OPS as i64)), rng);
		let nopa_e = ke.key_expr((p1 + p2).rem_euclid(KEY_MOD), None, rng);
		format!(
			"local OCt = {{}}\n  do\n    local ocp = {{{}}}\n    local oi = 1\n    while oi <= #ocp do\n      local x = ocp[oi]\n      OCt[x] = (ocp[oi + 1] - ({} + x * {}) % 65536) % 256\n      oi = oi + 2\n    end\n  end\n  local NOPA = ({}) % 256",
			pairs.join(", "), ocm_e, occ_e, nopa_e
		)
	} else {
		String::new()
	};
	// v15 ships the boot-built wire array; legacy keeps the named table
	let oc_table = if v15 { oc_boot } else { oc_table };

	let nop = handlers::nop::body(rng);
	// v15 stage E5: meta/type/error literals routed through the MS boot
	// table (numeric char codes); legacy keeps quoted literals verbatim
	let mut pool = StrPool::new(v15);
	// 建议1 (advanced): every build assigns each instruction ONE of its
	// formats at random — the interpreter text of the same opcode is
	// different across builds, all formats semantics-identical.
	let mut fmt_of: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
	for name in OP_NAMES.iter() {
		let n = handlers::n_formats(name, v15);
		fmt_of.insert((*name).to_string(), rng.int(0, (n - 1) as i64) as u8);
	}
	// v15 (A1 execution inlining): each dispatch leaf reads its operands
	// from the SoA streams and advances pc itself, so the loop head is the
	// sample's `local oc = W[pc]; if oc ...` shape (fingerprint F11).
	// Stream holding operand `op` is [SA,SB,SC,SD][operand_stream[op]],
	// where operand_stream[op] = the stream slot with slot_perm[sl]==op.
	let stream_names = ["SA", "SB", "SC", "SD"];
	let mut operand_stream = [0u8; 4];
	for (sl, &op_idx) in slot_perm.iter().enumerate() {
		operand_stream[op_idx as usize] = sl as u8;
	}
	let op_prefix = format!(
		"local a = {}[pc]; local b = {}[pc]; local c = {}[pc]; local d = {}[pc]; pc = pc + 1;",
		stream_names[operand_stream[0] as usize],
		stream_names[operand_stream[1] as usize],
		stream_names[operand_stream[2] as usize],
		stream_names[operand_stream[3] as usize],
	);
	let mut body_of = |name: &str| -> String {
		let core = if name == "Nop" || name == "NopA" {
			nop.clone()
		} else {
			// 建议1: fixed code returned by the instruction's own file
			handlers::gen(name, fmt_of[name], v15, &mut pool, mk)
		};
		if v15 {
			if core.is_empty() {
				op_prefix.clone()
			} else {
				format!("{} {}", op_prefix, core)
			}
		} else {
			core
		}
	};
	let mut items: Vec<(String, u8)> = (0..N_OPS)
		.map(|i| (OP_NAMES[i].to_string(), map.to_wire[i]))
		.collect();
	if v15 {
		// Nop alias leaf: a second wire value dispatching to the same
		// Nop body. The post-parse self-mod rewrites Nop sites between
		// the two encodings (bytecode mutates at load time; both decode
		// to Nop, so semantics are unchanged).
		items.push(("NopA".to_string(), map.nop_alias));
	}
	// 建议1 (advanced): dispatch leaf order is shuffled every generation
	// (wire codes are already a per-build permutation via OpMap).
	rng.shuffle(&mut items);
	let branches = gen_dispatch_tree(&items, &mut body_of, rng, 0);

	// v15 stage E3 (F13 + operand integrity): per-stream inline 7-bit
	// ladders. The sink closure maps the reconstructed value to its
	// destination (stream slot write, or the verify checksum fold).
	let ladder = |assign: &dyn Fn(&str) -> String| -> String {
		format!(
			"local b1 = BYTE(s, p)
          if b1 < 128 then {a1}; p = p + 1 else
            local b2 = BYTE(s, p + 1)
            if b2 < 128 then {a2}; p = p + 2 else
              local b3 = BYTE(s, p + 2)
              if b3 < 128 then local v = (b1 - 128) + (b2 - 128) * 128 + b3 * 16384; if v >= 2147483648 then v = v - 4294967296 end; {a3}; p = p + 3 else
                local b4 = BYTE(s, p + 3); local v = (b1 - 128) + (b2 - 128) * 128 + (b3 - 128) * 16384 + b4 * 2097152; if v >= 2147483648 then v = v - 4294967296 end; {a4}; p = p + 4
              end
            end
          end",
			a1 = assign("b1"),
			a2 = assign("(b1 - 128) + b2 * 128"),
			a3 = assign("v"),
			a4 = assign("v"),
		)
	};
	let stream_read = |name: &str| -> String {
		let sink: Box<dyn Fn(&str) -> String> =
			Box::new(move |x: &str| format!("{}[i] = {}", name, x));
		format!(
			"{} = {{}}
        for i = 1, ncode do
          {}
        end",
			name,
			ladder(&sink)
		)
	};
	let fold_read = || -> String {
		let sink: Box<dyn Fn(&str) -> String> = Box::new(|x: &str| {
			format!("ck = (ck + {}) % 4294967296", x)
		});
		format!(
			"for i = 1, ncode do
          {}
        end",
			ladder(&sink)
		)
	};

	// parse function: non-v15 keeps the original monolithic decode
	// byte-for-byte; v15 emits it as an explicit state-machine decode
	// (Phase B CPS foundation) with identical semantics.
	// P1 (致命缺点③): constant decode = LCG unmask (KM/KC per build,
	// cks = per-function seed in the blob header) + dyadic number
	// rebuild (type 4: m·2^k exact, no digit text in the blob).
	// 增量⑨ (防静态): the whole constant block is keystream-masked —
	// type byte, string length and payload all advance the LCG, so the
	// section has no cleartext structure. `mb()` = one masked byte read
	// (advance LCG + unmask), exactly mirroring the encoder byte order.
	// Numbers stay dyadic (type 4: m·2^k exact, no digit text in blob).
	let const_loop = r#"local function mb()
      cks = (CKM * cks + CKC) % 268435456
      local b = (BYTE(s, p) - ((cks % 256) + (FLR(cks / 256) % 256) + (FLR(cks / 65536) % 256) + FLR(cks / 16777216)) % 256) % 256
      p = p + 1
      return b
    end
    for i = 1, nconst do
      local t = mb()
      if t == 0 then
        C[i] = nil
      elseif t == 1 then
        C[i] = mb() == 1
      elseif t == 3 then
        local lo = mb()
        local hi = mb()
        local l = lo + hi * 256
        local xs = ""
        for j = 1, l do
          xs = xs .. CHAR(mb())
        end
        C[i] = xs
      else
        local m = 0
        local sh = 1
        while true do
          local bb = mb()
          if bb < 128 then m = m + bb * sh; break end
          m = m + (bb - 128) * sh
          sh = sh * 128
        end
        local kp = 0
        local sh2 = 1
        while true do
          local bb = mb()
          if bb < 128 then kp = kp + bb * sh2; break end
          kp = kp + (bb - 128) * sh2
          sh2 = sh2 * 128
        end
        local kk = FLR(kp / 2) - 2048
        local pw = 1
        if kk >= 0 then for j = 1, kk do pw = pw * 2 end else for j = 1, 0 - kk do pw = pw / 2 end end
        local v = m * pw
        if kp % 2 == 1 then v = 0 - v end
        C[i] = v
      end
    end"#;
	// 增量⑩: the constant/blob keystream keys are KF-assembled too.
	// NOTE: these six are evaluated at VM-BODY scope (the declarations
	// sit outside parse), so the recipes must stay anchor-free — no
	// parse-local names may appear in them.
	let ck_consts = format!(
		"local CKM = {}\n  local CKC = {}\n  local BKM = {}\n  local BKC = {}\n  local BSEED = {}\n  local BSTEP = {}\n  ",
		ke.key_expr(ck.0 as i64, None, rng),
		ke.key_expr(ck.1 as i64, None, rng),
		ke.key_expr(blobk.0 as i64, None, rng),
		ke.key_expr(blobk.1 as i64, None, rng),
		ke.key_expr(blobk.2 as i64, None, rng),
		ke.key_expr(blobk.3 as i64, None, rng),
	);
	manifest_key("CKM", ck.0 as u64);
	manifest_key("CKC", ck.1 as u64);
	manifest_key("BKM", blobk.0 as u64);
	manifest_key("BKC", blobk.1 as u64);
	manifest_key("BSEED", blobk.2 as u64);
	manifest_key("BSTEP", blobk.3 as u64);
	// P2 section tags (per-build identity bytes) + walk preamble:
	// position-unmask the whole blob, then tag-walk the sections in
	// blob order. Constant decoding is deferred to AFTER the walk
	// (the CKSEED section may follow CONSTS in the permutation).
	let tag_defs = format!(
		"local TH = {}\n    local TCK = {}\n    local TU = {}\n    local TC = {}\n    local TS = {}\n    local TDC = {}\n    local NSECT = {}\n    ",
		tags[0], tags[1], tags[2], tags[3], tags[4], tags[6], if v15 { 6 } else { 5 }
	);
	let unmask_pre = r#"s = decarrier(s)
    local n = #s
    local ksplit = n
    if ksplit > 64 then ksplit = 64 end
    local um = {}
    local g = (BSEED + (fi - 1) * BSTEP) % 268435456
    for i = 1, ksplit do
      g = (BKM * g + BKC) % 268435456
      um[i] = CHAR((BYTE(s, i) - (((g % 256) + (FLR(g / 256) % 256) + (FLR(g / 65536) % 256) + FLR(g / 16777216)) % 256)) % 256)
    end
    local hf = 0
    for i = 1, ksplit do
      hf = (hf * 31 + BYTE(s, i)) % 268435456
    end
    g = (BSEED + (fi - 1) * BSTEP + hf) % 268435456
    for i = ksplit + 1, n do
      g = (BKM * g + BKC) % 268435456
      um[i] = CHAR((BYTE(s, i) - (((g % 256) + (FLR(g / 256) % 256) + (FLR(g / 65536) % 256) + FLR(g / 16777216)) % 256)) % 256)
    end
    s = table.concat(um)
    __TAGS__local p = 1
    local got = 0
    local cstart = 0"#;
	let walk_head = r#"local tag = BYTE(s, p); p = p + 1
        if tag == TH then
          local v
          v, p = r16(s, p); nregs = v
          v, p = r16(s, p); nparams = v
          v, p = r16(s, p); vararg = v
          got = got + 1
        elseif tag == TCK then
          local v; v, p = r16(s, p); cks = v
          got = got + 1
        elseif tag == TU then
          local nu; nu, p = r16(s, p)
          upsrc = {}
          for i = 1, nu do local v; v, p = r16(s, p); upsrc[i] = v end
          got = got + 1
        elseif tag == TC then
          local v
          v, p = r16(s, p); nconst = v
          v, p = r16(s, p); csl = v
          cstart = p
          p = p + csl
          got = got + 1
        elseif tag == TS then
          local ns; ns, p = r16(s, p)
          S = {}
          for i = 1, ns do local v; v, p = r16(s, p); S[i] = v end
          got = got + 1
        elseif tag == TDC then
          local dl; dl, p = r16(s, p)
          p = p + dl
        else
          local v; v, p = r16(s, p); ncode = v
          W = {}
          for i = 1, ncode do W[i] = BYTE(s, p); p = p + 1 end
          __CODE_EXTRA__
          got = got + 1
        end"#;
	let parse_fn = if v15 {
		let code_extra = r#"local p0 = p
          __STREAMS__
          local pend = p
          p = p0
          local ck = 0
          __FOLDS__
          CK = ck
          p = pend"#;
		String::from(
			r#"local function parse(s, fi)
    __UNMASK__
    local st = 1
    local nregs, nparams, vararg, cks, csl, upsrc, nconst, C, S, ncode, W, SA, SB, SC, SD, CK
    while st <= 2 do
      if st == 1 then
        __WALK__
        if got >= NSECT then st = 2 end
      else
        p = cstart
        C = {}
        __CONSTS__
        if p ~= cstart + csl then while true do end end
        st = 3
      end
    end
    return { nregs = nregs, nparams = nparams, vararg = vararg, upsrc = upsrc, C = C, S = S, ck = CK, W = W, SA = SA, SB = SB, SC = SC, SD = SD }
  end"#
            .replace("__UNMASK__", unmask_pre)
            .replace("__TAGS__", &tag_defs)
            .replace("__WALK__", walk_head)
            .replace("__CODE_EXTRA__", code_extra)
            .replace("__CONSTS__", const_loop)
            .replace(
                "__STREAMS__",
                &format!(
                    "{}\n        {}\n        {}\n        {}",
                    stream_read("SA"),
                    stream_read("SB"),
                    stream_read("SC"),
                    stream_read("SD")
                ),
            )
            .replace(
                "__FOLDS__",
                &format!(
                    "{}\n        {}\n        {}\n        {}",
                    fold_read(),
                    fold_read(),
                    fold_read(),
                    fold_read()
                ),
            ),
		)
	} else {
		let code_extra =
			r#"SA, SB, SC, SD = rstream(), rstream(), rstream(), rstream()"#;
		String::from(
			r#"local function parse(s, fi)
    __UNMASK__
    local nregs, nparams, vararg, cks, csl, upsrc, nconst, C, ncode, W, SA, SB, SC, SD
    local function rstream()
      local T = {}
      for i = 1, ncode do
        local v, np = r16(s, p); p = np; T[i] = v
      end
      return T
    end
    while got < NSECT do
      __WALK__
    end
    p = cstart
    C = {}
    __CONSTS__
    if p ~= cstart + csl then while true do end end
    return { nregs = nregs, nparams = nparams, vararg = vararg, upsrc = upsrc, C = C, W = W, SA = SA, SB = SB, SC = SC, SD = SD }
  end"#,
		)
		.replace("__UNMASK__", unmask_pre)
		.replace("__TAGS__", &tag_defs)
		.replace("__WALK__", walk_head)
		.replace("__CODE_EXTRA__", code_extra)
		.replace("__CONSTS__", const_loop)
	};

	// v15 (Phase B): the carrier->prototype decode is emitted as its own
	// state-machine segment (an explicit decode-state loop) instead of the
	// shared `for` loop, establishing the separated decode stage that the
	// CPS pipeline builds on. Non-v15 keeps the original `for` byte-for-byte.
	let decode_seg = if v15 {
		// 增量⑭ (对抗 R002): the decode loop + OS checksum table moved
		// INTO the encrypted bootstrap fragment (built in the hfrag
		// section below). No visible parse loop, no visible checksum
		// oracle — the analyst must break HQ encryption first.
		String::new()
	} else {
		String::from("for i = 1, #FN do PF[i] = parse(FN[i], i) end")
	};

	// 增量⑭: v15 emits parse_fn invisibly (inside the bootstrap
	// fragment); legacy keeps it in the visible body.
	let parse_fn_vis = if v15 { String::new() } else { parse_fn.clone() };

	// v15 (P3-B): bytecode self-modification + dead dispatch segment.
	//   self-mod: the compiler reports every Nop site; at load time each
	//   is rewritten to the Nop-alias wire value via a LITERAL constant
	//   write (sample `J[Q]=12` shape, fingerprints F14/F27). The
	//   primary Nop wire becomes dead (decoy), the alias carries every
	//   dead instruction, and opcode encoding is unstable within the
	//   stream. Both writes and semantics are neutral (Nop -> Nop).
	//   dead segment: a site1-shaped fetch tree (sample's never-hit
	//   decode path) guarded by an always-false flag.
	// ㉒ (B-2 碎片即用即毁): the Nop-alias self-modification writes
	// move INSIDE the boot `do` block (emitted into the `build` string
	// below) — they must land while PF is still decoded, right before
	// the ENC fragment encodes every prototype into ciphertext. The
	// visible F14/F27 shape is preserved; only its home shifts a few
	// lines earlier.
	let mut v15_zw = String::new();
	let v15_selfmod = if v15 {
		let mut sm = String::new();
		for (fi, sites) in nop_sites.iter().enumerate() {
			if sites.is_empty() {
				continue;
			}
			// bind the opcode array to a local and write through it
			// (sample shape: direct array constant writes, F14)
			v15_zw.push_str(&format!("  local ZW{} = PF[{}].W\n", fi + 1, fi + 1));
			for &p in sites {
				v15_zw.push_str(&format!(
					"  ZW{}[{}] = NOPA\n",
					fi + 1,
					p as usize + 1,
				));
			}
		}
		// 增量⑫ (防静态, 报告突破口 #14): the dead-dispatch decoy used a
		// literal `DF = 0`, so static dead-code elimination proved the
		// loop unreachable and discarded it (zero-reference decoy spotted).
		// 增量⑬ (对抗 R002 §3.37): the attacker proved `ck % 1 == 0`
		// algebraically and discarded the whole decoy stage. Derive DF
		// from the key-lookup tables instead: three reserved KA/KB/KC
		// entries sum to 0 mod 7 BY CONSTRUCTION, so DF is still 0 at
		// runtime, but proving it dead now requires fetching those table
		// values through obfuscated indexes — no universal identity to
		// fold away.
		let df_e = ke.dead_zero(rng);
		sm.push_str(&format!("  local DA = {{}}\n  local DP = 1\n  local DF = {}\n  while DF > 0 do\n    local f = DA[DP]\n    if f >= 4 then\n      if f < 6 then\n        if f ~= 5 then DF = 0 else DF = 0 end\n      else DF = 0 end\n    elseif f < 2 then DF = 0\n    else DF = 0 end\n", df_e));
		// P3c: decoy fetch points — the sample ships several dispatch
		// loops (golden F11 = 19); these dead fetch shapes raise the
		// family resemblance and multiply the "which loop is real"
		// question for an analyst. Never executed (DF stays 0).
		// 增量⑲: the REAL fetch left the visible surface (encrypted
		// chain epilogues), so the decoys alone must keep the F11 band
		// (>= 4) — and they are now the ONLY visible fetch shapes, which
		// makes "which loop is real" a dead-end by construction.
		let n_decoys = rng.int(4, 6);
		for _ in 0..n_decoys {
			let i1 = rng.int(0, (N_OPS - 1) as i64);
			let i2 = rng.int(0, (N_OPS - 1) as i64);
			sm.push_str(&format!(
				"    local oc = DA[DP]\n    if oc then\n      local a = DA[DP]\n      local b = DA[DP]\n      local c = DA[DP]\n      local d = DA[DP]\n      DP = DP + 1\n      if oc == OCt[{}] then DA[DP] = a elseif oc == OCt[{}] then DA[DP] = b else local r = DA[DP](a, b, c, d); if r then DF = r[1] end end\n    end\n",
				i1, i2
			));
		}
		sm.push_str("    DP = DP + 1\n  end\n");
		// 存量缺陷清理（R007 回合）：boot 完成后钥匙表与解析钥匙已无
		// 引用（DF 诱饵在本行之前求值完毕），全部抹除——内存转储拿不
		// 到任何钥匙装配材料。（注意：这些都是 VM 体局部，mangle 后
		// 名字随机；nil 赋值经管线无损。）
		sm.push_str("  KA = nil; KB = nil; KC = nil; KM = nil\n");
		sm.push_str("  CKM = nil; CKC = nil; BKM = nil; BKC = nil; BSEED = nil; BSTEP = nil\n");
		sm.push_str("  OCt = nil\n");
		sm
	} else {
		String::new()
	};

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
	let mut run_unpack = format!(
		"local {} = {}",
		run_lhs.join(", "),
		run_rhs.join(", ")
	);

	// carrier decoder tables
	// 增量⑨-2 (防静态): the base-94 alphabet is NO LONGER a clean
	// `AL[byte]=idx` table (the attack recognized the 94-entry table
	// instantly and bootstrapped the whole decode from it). Instead the
	// ordered alphabet is stored LCG-masked and the reverse map is built
	// at boot, so the byte->index table never appears in the output.
	let akm = (rng.int(1_048_577, 33_000_001) | 1) as u32;
	let akc = (rng.int(1_048_576, 268_000_000) | 1) as u32;
	let aseed = rng.int(1_048_576, 268_435_455) as u32;
	let mut astate = aseed as u64;
	let masked_alpha: Vec<String> = carrier
		.alphabet
		.iter()
		.map(|&ch| {
			astate = (akm as u64 * astate + akc as u64) % 268_435_456;
			ch.wrapping_add(fold_key(astate)).to_string()
		})
		.collect();
	// 增量⑩: rebuild keys KF-assembled, anchored on #APH (the masked
	// byte table sitting right above the rebuild loop).
	let aseed_e = ke.key_expr(aseed as i64, Some(("#APH", 94)), rng);
	let akm_e = ke.key_expr(akm as i64, Some(("#APH", 94)), rng);
	let akc_e = ke.key_expr(akc as i64, Some(("#APH", 94)), rng);
	manifest_key("APH_SEED", aseed as u64);
	manifest_key("APH_KM", akm as u64);
	manifest_key("APH_KC", akc as u64);
	let al_lines = format!(
		"local APH = {{{}}}\n  local AL = {{}}\n  do\n    local ast = {}\n    for i = 1, 94 do\n      ast = ({} * ast + {}) % 268435456\n      AL[(APH[i] - (((ast % 256) + (FLR(ast / 256) % 256) + (FLR(ast / 65536) % 256) + FLR(ast / 16777216)) % 256)) % 256] = i - 1\n    end\n  end\n",
		masked_alpha.join(", "), aseed_e, akm_e, akc_e
	);
	// 增量⑨-2 (防静态): the 10-token escape table is also stored
	// masked and rebuilt at boot (no `TK[CHAR(...)]=CHAR(...)` rows).
	let tkm = (rng.int(1_048_577, 33_000_001) | 1) as u32;
	let tkc = (rng.int(1_048_576, 268_000_000) | 1) as u32;
	let tkseed = rng.int(1_048_576, 268_435_455) as u32;
	let mut tkstate = tkseed as u64;
	let mut tk_bytes: Vec<u8> = Vec::new();
	for i in 0..10 {
		tk_bytes.extend_from_slice(carrier.tokens[i].as_bytes());
	}
	tk_bytes.extend_from_slice(&CARRIER_SPECIALS);
	let masked_tk: Vec<String> = tk_bytes
		.iter()
		.map(|&b| {
			tkstate = (tkm as u64 * tkstate + tkc as u64) % 268_435_456;
			b.wrapping_add(fold_key(tkstate)).to_string()
		})
		.collect();
	// 增量⑩: token-table rebuild keys KF-assembled, anchored on #TKD.
	let tkseed_e = ke.key_expr(tkseed as i64, Some(("#TKD", 60)), rng);
	let tkm_e = ke.key_expr(tkm as i64, Some(("#TKD", 60)), rng);
	let tkc_e = ke.key_expr(tkc as i64, Some(("#TKD", 60)), rng);
	manifest_key("TK_SEED", tkseed as u64);
	manifest_key("TK_KM", tkm as u64);
	manifest_key("TK_KC", tkc as u64);
	let tk_lines = format!(
		"local TKD = {{{}}}\n  local TK = {{}}\n  do\n    local tst = {}\n    local tb = {{}}\n    for i = 1, 60 do\n      tst = ({} * tst + {}) % 268435456\n      tb[i] = (TKD[i] - (((tst % 256) + (FLR(tst / 256) % 256) + (FLR(tst / 65536) % 256) + FLR(tst / 16777216)) % 256)) % 256\n    end\n    for i = 0, 9 do\n      TK[CHAR(tb[i * 5 + 1], tb[i * 5 + 2], tb[i * 5 + 3], tb[i * 5 + 4], tb[i * 5 + 5])] = CHAR(tb[50 + i + 1])\n    end\n  end\n",
		masked_tk.join(", "), tkseed_e, tkm_e, tkc_e
	);

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
	// v15 (A1): inline sample-shape fetch — `local oc = W[pc]` with no
	// pc-advance / operand-bind (each dispatch leaf does that via
	// op_prefix), giving the loop head the sample's F11 shape. The hub
	// machinery stays for the legacy profile only.
	let (hub_decl, fetch) = if v15 {
		(String::new(), "local oc = W[pc]".to_string())
	} else if use_hub {
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
	if v15 {
		// stage A: the per-function register slot table S joins the
		// state tuple (scattered register layout)
		fields.push("S");
	}
	rng.shuffle(&mut fields);
	let mut run_soa = format!(
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

	// 增量⑲ (递归帧预算): Luau 每调用积 1 个 C 侧帧、~20k 帧溢栈
	// （无 TCO，devforum 3307934）。旧设计每层递归 4 持久帧（闭包+
	// run+callcap+w）恰好贴线过 tail(5000)。新架构必须 ≤4：蹦床循环
	// 直接内联进 makefn 闭包（省掉 wrapper 之外的 run 帧），v15
	// callcap 用 table.pack 省掉 w 帧（见 rt_helpers）。
	let trampoline_src = if v15 {
		const CHAIN_BUDGET: i64 = 16;
		let budget_e = obf_num(CHAIN_BUDGET as u64, rng);
		Some(format!(
			"local ti = 0\n    while true do\n      E.b = {}\n      if E.callx > 0 then\n        local cx = CX[E.callx]\n        E.callx = 0\n        cx(E)\n      else\n        CT[(ti % 4) + 1](E)\n        ti = ti + 1\n      end\n      if E.done then return U(E.out, 1, E.total) end\n    end",
			budget_e
		))
	} else {
		None
	};

	// makefn: v15 stage A variant translates upvalue descriptors and
	// parameter fills through the scattered slot tables (the PARENT's
	// S resolves upsrc register references; the child's pf.S maps the
	// parameter registers). Legacy keeps the dense layout.
	let makefn_decl = if v15 {
		String::from(
			"local function makefn(pf, V, upsf, S)
    local c = {}
    for i = 1, #pf.upsrc do
      local src = pf.upsrc[i]
      if src >= 49152 then
        c[i] = upsf[src - 49152]
      elseif src >= 32768 then
        c[i] = { v = V[S[src - 32768]], i = 1 }
      else
        c[i] = { v = V, i = S[src] }
      end
    end
    return function(...)
      local all = { ... }
      local vargc = #all - pf.nparams
      if vargc < 0 then vargc = 0 end
      local vargs = {}
      for i = 1, vargc do vargs[i] = all[pf.nparams + i] end
      local V2 = {}
      for i = 1, pf.nparams do V2[pf.S[i]] = all[i] end
      -- 增量⑲: the trampoline lives INSIDE the closure (one frame per
      -- recursion level instead of closure+run)
      local E = newE(pf, V2, c, vargs, vargc)
      TRAMPOLINE
    end
  end",
		)
		.replace("TRAMPOLINE", trampoline_src.as_deref().unwrap())
	} else {
		String::from(
			"local function makefn(idx, V, upsf)
    local pf = PF[idx]
    local c = {}
    for i = 1, #pf.upsrc do
      local src = pf.upsrc[i]
      if src >= 49152 then
        c[i] = upsf[src - 49152]
      elseif src >= 32768 then
        c[i] = { v = V[src - 32768], i = 1 }
      else
        c[i] = { v = V, i = src }
      end
    end
    return function(...)
      local all = { ... }
      local vargc = #all - pf.nparams
      if vargc < 0 then vargc = 0 end
      local vargs = {}
      for i = 1, vargc do vargs[i] = all[pf.nparams + i] end
      local V2 = {}
      for i = 1, pf.nparams do V2[i] = all[i] end
      -- real TCO: tail call into run so deep tail recursion reuses the
      -- frame instead of stacking one per level
      return run(pf, V2, c, vargs, vargc)
    end
  end",
		)
	};

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

	// v15 Phase C (redo on TCO foundation): CPS execution dispatch. Each
	// opcode is a handler in H, called from the loop. The Return handler
	// returns a signal {out, total}; the loop unpacks it via a TAIL call
	// `return U(r[1], 1, r[2])` so run still returns unpacked results
	// (consistent with real TCO). With real TCO the closure->run frame is
	// reused, reducing per-recursion-level stack growth.
	//
	// P3a (致命缺点①): the handler bodies no longer appear as code. Each
	// is wrapped into an env-parameterized closure source, LCG-masked,
	// base-94 packed and stored as a long-string HQ fragment; boot
	// decodes + `loadstring`s them into HW (wire -> function), after a
	// loadstring-nativeness recheck (hooked loader -> silent trap).
	//
	// 增量⑲ (选项B路线二 — CPS 去中心化尾调用链, 对抗 R006): the
	// central `while true do fetch dispatch end` loop is GONE. Every
	// handler ends with its own fetch+dispatch epilogue that TAIL-CALLS
	// the next handler (pc lives in E.pc); the Return handler exits the
	// chain with `return U(out, 1, total)`, whose values propagate back
	// through the all-tail-called chain with zero frame accumulation.
	// Dispatch is now scattered across 43 per-build encrypted handler
	// bodies — no single hookable choke point remains (R006 barrier ②:
	// 全局 hook 咽喉点消失). Handlers therefore need the streams +
	// dispatch table in their env.
	// 增量⑲: operand k lives in stream operand_stream[k] (slot_perm
	// permutation) — the chain epilogues must fetch (a,b,c,d) through
	// the SAME mapping the old op_prefix used.
	let s_of = [
		stream_names[operand_stream[0] as usize],
		stream_names[operand_stream[1] as usize],
		stream_names[operand_stream[2] as usize],
		stream_names[operand_stream[3] as usize],
	];
	let env_names = [
		"V", "C", "S", "O", "G", "vargs", "vargc", "ups", "makefn", "mget",
		"resolve_call", "callcap", "HAS_LEN_META", "CHAR", "FLOOR", "ERR",
		"TYP", "GMT", "RGET", "RSET", "U", "MS",
		"W", "SA", "SB", "SC", "SD", "AV", "HW2",
	];
	let prelude_lhs = env_names.join(", ");
	let prelude_rhs: Vec<String> =
		(0..env_names.len()).map(|i| format!("E[{}]", i + 1)).collect();
	let prelude_rhs = prelude_rhs.join(", ");
	let e_table = format!(
		"{{{}, k = pf.k, ln = 0, lb = 0, pc = 1, b = 0, callx = 0, done = false, out = {{}}, total = 0, tp = TP}}",
		env_names.join(", ")
	);
	let hfrag: String;
	let handler_defs = if v15 {
		let hm = (rng.int(1_048_577, 33_000_001) | 1) as u32;
		let hc = (rng.int(1_048_576, 268_000_000) | 1) as u32;
		let hseed = rng.int(1_048_576, 268_435_455) as u32;
		let mut frags: Vec<(u16, Vec<u8>)> = Vec::new(); // (wire, source bytes)
		// 增量⑲ (CPS 去中心化, Call 浅化): Luau 无正确尾调用，调用点
		// 若在链深处执行，用户递归每层会叠 ~K 个链帧 → 深递归溢栈。
		// 故 Call 家族拆两相：链相位（参数打包 → 暂存 E → return 解链
		// 回蹦床）+ 执行器碎片（蹦床在浅层调用，callcap 之后接标准尾
		// 链附言继续执行）。递归帧剖面回到旧设计量级，而分派逻辑仍全
		// 在加密碎片里。执行器 wire 305-308（避开 0..255 操作数 wire
		// 与 BSS 标记 200）。
		let call_names = ["Call", "CallE", "CallM", "CallT"];
		// 增量⑲ (递归帧瘦身): Luau 无 TCO，用户递归每层叠数个链帧，
		// 每帧的 env prelude 绑定是栈槽大头。按 body（含尾链附言）实际
		// 引用做**选择性绑定**——平均帧槽从 29 降到个位数，深递归极限
		// 抬升数倍。word-boundary 判定（std-only，无 regex crate）。
		fn word_in(h: &str, n: &str) -> bool {
			let hb = h.as_bytes();
			let nb = n.as_bytes();
			let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
			let mut start = 0usize;
			while start + nb.len() <= hb.len() {
				match h[start..].find(n) {
					Some(off) => {
						let i = start + off;
						let before_ok = i == 0 || !is_ident(hb[i - 1]);
						let j = i + nb.len();
						let after_ok = j >= hb.len() || !is_ident(hb[j]);
						if before_ok && after_ok {
							return true;
						}
						start = i + 1;
					}
					None => return false,
				}
			}
			false
		}
		let selective_prelude = |body: &str| -> String {
			let mut lhs: Vec<&str> = Vec::new();
			let mut rhs: Vec<String> = Vec::new();
			for (i, nm) in env_names.iter().enumerate() {
				if word_in(body, nm) {
					lhs.push(nm);
					rhs.push(format!("E[{}]", i + 1));
				}
			}
			if lhs.is_empty() {
				String::new()
			} else {
				format!("local {} = {} ", lhs.join(", "), rhs.join(", "))
			}
		};
		// word-boundary replace (std-only, no regex crate)
		fn replace_word(s: &str, from: &str, to: &str) -> String {
			let mut out = String::with_capacity(s.len());
			let mut rest = s;
			while let Some(i) = rest.find(from) {
				let before_ok = i == 0
					|| !rest.as_bytes()[i - 1].is_ascii_alphanumeric()
						&& rest.as_bytes()[i - 1] != b'_';
				let end = i + from.len();
				let after_ok = end == rest.len()
					|| !rest.as_bytes()[end].is_ascii_alphanumeric()
						&& rest.as_bytes()[end] != b'_';
				out.push_str(&rest[..i]);
				if before_ok && after_ok {
					out.push_str(to);
				} else {
					out.push_str(from);
				}
				rest = &rest[end..];
			}
			out.push_str(rest);
			out
		}
		for (name, wire) in &items {
			let mut body = if name == "Nop" || name == "NopA" {
				nop.clone()
			} else {
				handlers::gen(name, fmt_of[name], true, &mut pool, mk)
			};
			body = body.replace("lastbase", "E.lb").replace("lastn", "E.ln");
			// 增量⑲ (CPS 去中心化尾调用链): the central dispatch loop is
			// gone — every non-Return handler ends with its own
			// fetch+dispatch epilogue tail-calling the next handler (pc
			// threads through E.pc); jumps write E.pc in place; Return
			// exits the chain with `return U(out, 1, total)`, whose
			// values propagate back through the all-tail-called chain.
			// The Call family is NO LONGER inline: with real tail calls
			// its handler frames never accumulate, so the four Call
			// handlers run from their HQ fragments like any other
			// opcode (they were inlined only to dodge per-instruction
			// frame growth in the old loop — and ⑬'s equivalence-
			// breaking junk dies with the decoy status).
			if matches!(name.as_str(), "Jmp" | "Jf" | "Jt") {
				body = body.replace("pc = b", "E.pc = b");
			}
			if name == "Return" {
				// 增量⑲: the chain cannot hand results back through a
				// bounded-depth unwind — Return stores them in E and
				// sets the done flag; the trampoline exits with
				// U(E.out, 1, E.total).
				body = body
					.replace(
						"return U(out, 1, total)",
						"E.out = out; E.total = total; E.done = true; return",
					)
					.replace(
						"return U(ov, 1, tot)",
						"E.out = ov; E.total = tot; E.done = true; return",
					);
			} else if let Some(ci) =
				call_names.iter().position(|n| *n == name.as_str())
			{
				// ---- split the Call body at the callcap invocation ----
				let marker = ", nout = callcap(fn, ";
				let mi = body.find(marker).expect("callcap marker");
				let ls = body[..mi].rfind("local ").expect("callcap local");
				let (pre, post) = body.split_at(ls);
				let after = &body[mi + marker.len()..];
				let aname = after[..after.find(", nargs)").unwrap()].to_string();
				let mut stash = format!(
					"E.cf = fn; E.ca = {}; E.cn = nargs; E.oa = a; E.ob = b; E.oc2 = c; E.od = d",
					aname
				);
				let mut post_s = post.to_string();
				let mut exec_pre = String::new();
				// «B» base local (Call/CallE/CallM): recompute in the
				// executor from the stashed `a` operand
				if let Some(bi) = pre.find(" = a + 1;") {
					let bstart = pre[..bi].rfind("local ").unwrap() + 6;
					let bname = &pre[bstart..bi];
					exec_pre = format!("local {} = a + 1; ", bname);
				}
				if name == "CallT" {
					// «T»/«N» are read BEFORE the call but consumed after
					// — the call may mutate V, so stash the VALUES
					let p1 = pre.find(" = V[b + 1]; local ").unwrap();
					let tstart = pre[..p1].rfind("local ").unwrap() + 6;
					let tname = pre[tstart..p1].to_string();
					let p2 = pre.find(" = V[c + 1]").unwrap();
					let nstart = pre[..p2].rfind("local ").unwrap() + 6;
					let nname = pre[nstart..p2].to_string();
					stash.push_str(&format!(
						"; E.ctt = {}; E.cnt = {}",
						tname, nname
					));
					post_s = replace_word(&post_s, &tname, "E.ctt");
					post_s = replace_word(&post_s, &nname, "E.cnt");
				}
				// 增量⑲ (帧预算): inline the multi-return capture into
				// the executor (table.pack via E.tp) — the separate
				// callcap frame would persist through the whole nested
				// call (recursion frame budget).
				let split_marker =
					format!(", nout = callcap(fn, {}, nargs)", aname);
				let si = post_s.find(&split_marker).expect("callcap head");
				let oname = post_s["local ".len()..si].to_string();
				post_s = format!(
					"local {} = E.tp(fn(U(E.ca, 1, E.cn))); local nout = {}.n{}",
					oname,
					oname,
					&post_s[si + split_marker.len()..]
				);
				// executor fragment: shallow call + result placement +
				// chain continuation
				let exec_src = format!(
					"return function(E) local a, b, c, d = E.oa, E.ob, E.oc2, E.od; local fn, {}, nargs = E.cf, E.ca, E.cn; local V, S, O, U, W, SA, SB, SC, SD, AV, HW2 = E[1], E[3], E[4], E[21], E[23], E[24], E[25], E[26], E[27], E[28], E[29]; {}{} {} end",
					aname,
					exec_pre,
					post_s,
					chain_epilogue(rng, s_of[0], s_of[1], s_of[2], s_of[3]),
				);
				frags.push(((305 + ci) as u16, exec_src.into_bytes()));
				// chain phase: pack args, stash, unwind to the trampoline
				body = format!(
					"{}; E.callx = {}; return",
					format!("{};{}", pre.trim_end_matches(|c| c == ';' || c == ' '), stash),
					ci + 1
				);
			} else {
				body.push(' ');
				body.push_str(&chain_epilogue(rng, s_of[0], s_of[1], s_of[2], s_of[3]));
			}
			let src = format!(
				"return function(E,a,b,c,d) {}{} end",
				selective_prelude(&body),
				body
			);
			frags.push((*wire as u16, src.into_bytes()));
		}
		// 增量⑲ (蹦床重入点入密): the chain re-entry points are
		// continuation fragments — epilogue-shaped bodies (budget +
		// fetch E.pc + dispatch) with NO opcode semantics, emitted as
		// four more encrypted fragments (wires 201-204). The trampoline
		// loop rotates through them; nothing in the visible code ever
		// fetches W[..] or touches HW2.
		let n_cont = 4usize;
		for i in 0..n_cont {
			let body =
				chain_epilogue(rng, s_of[0], s_of[1], s_of[2], s_of[3]);
			let src = format!(
				"return function(E) local W, SA, SB, SC, SD, AV, HW2 = E[23], E[24], E[25], E[26], E[27], E[28], E[29] {} end",
				body
			);
			// 301+ clears every opcode wire (0..=255) and the BSS marker 200
			frags.push((301 + i as u16, src.into_bytes()));
		}
		// 增量⑭ (对抗 R002 — 解析器隐藏): the prototype parser + the
		// whole decode/checksum-verify stage move into ONE MORE
		// encrypted HQ fragment (wire marker 200). What R002 ported
		// straight out of the visible text (section tags, LCG unmask,
		// varint ladders, constant decode, and the OS checksum oracle)
		// now exists only inside the same encryption as the handlers —
		// the visible interpreter shrinks to the boot stub.
		let os_list = operand_sums
			.iter()
			.map(|s| obf_num(*s, rng))
			.collect::<Vec<_>>()
			.join(", ");
		let bs_src = format!(
			"return function(BYTE, CHAR, FLR, SUB, AL, TK, decarrier, r16, CKM, CKC, BKM, BKC, BSEED, BSTEP) local OS = {{{}}} {} return function(FN_) local PF_ = {{}} local di = 1 while di <= #FN_ do PF_[di] = parse(FN_[di], di); if PF_[di].ck ~= OS[di] then while true do end end; di = di + 1 end return PF_ end end",
			os_list, parse_fn
		);
		frags.push((200u16, bs_src.into_bytes()));
		// ㉒ (选项B — B-2 碎片即用即毁, R006 壁垒①): prototypes live
		// ENCODED at rest. Boot parses PF exactly as before, then the
		// ENC fragment (wire 207) masks every prototype's five streams
		// + constant pool with a per-prototype LCG keystream, links the
		// parent→child prototype tree through encrypted handles, and
		// returns only the tree root — the flat decoded PF is destroyed
		// in the same breath. Each call decodes ONE frame's worth of
		// bytecode through the DDEC fragment (wire 206) into fresh
		// tables owned by the frame; when the frame unwinds, decoded
		// bytecode becomes garbage. No moment in the process lifetime
		// holds the complete decoded program: resident state shrinks to
		// ciphertext + live frames (HW2/CT/CX executable handlers stay
		// resident — dispatch hot path).
		let pseed = rng.int(1_048_576, 268_435_455) as i64;
		let pstep = rng.int(1_048_576, 268_435_455) as i64;
		let pkm = (rng.int(1_048_577, 33_000_001) | 1) as i64;
		let pkc = rng.int(1_048_576, 268_000_000) as i64;
		let pseed_e = ke.key_expr(pseed, None, rng);
		let pstep_e = ke.key_expr(pstep, None, rng);
		let pkm_e = ke.key_expr(pkm, None, rng);
		let pkc_e = ke.key_expr(pkc, None, rng);
		manifest_key("PF_SEED", pseed as u64);
		manifest_key("PF_STEP", pstep as u64);
		manifest_key("PF_KM", pkm as u64);
		manifest_key("PF_KC", pkc as u64);
		// operand-b stream (carries the Closure child index) + the
		// Closure wire byte — both baked into the ENC scan
		let closure_wire = map.to_wire
			[OP_NAMES.iter().position(|n| *n == "Closure").unwrap()];
		// ENC: P = decoded prototype list, NOPA = self-mod alias (ZW
		// writes already applied visibly in the boot block), FLR/TYP/
		// BYTE passed from boot locals. Stream entries are u16; the
		// keystream advances once per masked unit. Constants: type
		// folded into the masked type slot (0=nil 1=num 2=str 3=true
		// 4=false); numbers masked additively (exact when consts_safe);
		// string bytes masked per-byte.
		let c_block_enc = if consts_safe {
			format!(
				"local Ct = pf.C local m = 0 for j in pairs(Ct) do if j > m then m = j end end local ct, cn, csb, csl = {{}}, {{}}, {{}}, {{}} local si = 1 for j = 1, m do st = ({pkm} * st + {pkc}) % 268435456 local v = Ct[j] local tv = TYP(v) if tv == \"number\" then if v % 1 == 0 and v > -1125899906842624 and v < 1125899906842624 then ct[j] = (1 + st) % 65536 cn[j] = v + st else local ns = TOSTR(v) ct[j] = (5 + st) % 65536 csl[j] = #ns for cb = 1, #ns do st = ({pkm} * st + {pkc}) % 268435456 csb[si] = (BYTE(ns, cb) + st % 256) % 256 si = si + 1 end end elseif tv == \"string\" then ct[j] = (2 + st) % 65536 csl[j] = #v for cb = 1, #v do st = ({pkm} * st + {pkc}) % 268435456 csb[si] = (BYTE(v, cb) + st % 256) % 256 si = si + 1 end elseif tv == \"boolean\" then if v then ct[j] = (3 + st) % 65536 else ct[j] = (4 + st) % 65536 end else ct[j] = st % 65536 end end",
				pkm = pkm_e, pkc = pkc_e,
			)
		} else {
			String::from(
				"local Ct = pf.C local m = 0 for j in pairs(Ct) do if j > m then m = j end end local ct, cn, csb, csl = nil, nil, nil, nil",
			)
		};
		let c_fields_enc = if consts_safe {
			"m = m, ct = ct, cn = cn, csb = csb, csl = csl"
		} else {
			"m = m, C = Ct"
		};
		let enc_src = format!(
			"return function(P, FLR, TYP, BYTE, TOSTR, KA, KB, KC, KM) local EP = {{}} for i = 1, #P do local pf = P[i] local Wt, ks = pf.W, pf.{bs} local n = #Wt local km = {{}} local st = ({pseed} + i * {pstep}) % 268435456 local e = {{}} local ei = 1 for j = 1, n do local w = Wt[j] if w == {cw} then km[#km + 1] = ks[j] + 1 end st = ({pkm} * st + {pkc}) % 268435456 e[ei] = (w + st % 65536) % 65536 ei = ei + 1 st = ({pkm} * st + {pkc}) % 268435456 e[ei] = (pf.SA[j] + st % 65536) % 65536 ei = ei + 1 st = ({pkm} * st + {pkc}) % 268435456 e[ei] = (pf.SB[j] + st % 65536) % 65536 ei = ei + 1 st = ({pkm} * st + {pkc}) % 268435456 e[ei] = (pf.SC[j] + st % 65536) % 65536 ei = ei + 1 st = ({pkm} * st + {pkc}) % 268435456 e[ei] = (pf.SD[j] + st % 65536) % 65536 ei = ei + 1 end {cblock} EP[i] = {{ e = e, n = n, sd = i, S = pf.S, upsrc = pf.upsrc, nparams = pf.nparams, k = {{}}, km = km, {cfields} }} end for i = 1, #EP do local km = EP[i].km for j = 1, #km do EP[i].k[km[j]] = EP[km[j]] end EP[i].km = nil end return EP[#EP] end",
			bs = s_of[1],
			pseed = pseed_e,
			pstep = pstep_e,
			pkm = pkm_e,
			pkc = pkc_e,
			cw = closure_wire,
			cblock = c_block_enc,
			cfields = c_fields_enc,
		);
		// DDEC: per-call frame decode (fresh tables; the frame owns
		// them and they die with it). Mirrors the ENC keystream exactly.
		let c_block_dec = if consts_safe {
			String::from(
				"local C = {} local ct, cn, csb, csl = q.ct, q.cn, q.csb, q.csl local si = 1 for j = 1, q.m do st = (KM * st + KC) % 268435456 local t = (ct[j] - st) % 65536 if t == 1 then C[j] = cn[j] - st elseif t == 2 or t == 5 then local l = csl[j] local b = {} for x = 1, l do st = (KM * st + KC) % 268435456 b[x] = (csb[si] - st % 256) % 256 si = si + 1 end if t == 2 then C[j] = CHAR(UNP(b, 1, l)) else C[j] = TONUM(CHAR(UNP(b, 1, l))) end elseif t == 3 then C[j] = true elseif t == 4 then C[j] = false end end",
			)
		} else {
			String::from("local C = q.C")
		};
		// NOTE: DDEC runs on EVERY call — long after the boot cleanup
		// nils the KA/KB/KC key tables. The keystream constants are
		// therefore injected as BOOT-TIME closure upvalues (factory
		// form): the key-fragment expressions evaluate once while KA is
		// alive, the compiled decoder captures four plain numbers.
		let dec_src = format!(
			"return function(PS, PT, KM, KC) return function(q, FLR, CHAR, UNP, TONUM) local st = (PS + q.sd * PT) % 268435456 local n = q.n local W, SA, SB, SC, SD = {{}}, {{}}, {{}}, {{}}, {{}} local e = q.e local ei = 1 for i = 1, n do st = (KM * st + KC) % 268435456 W[i] = (e[ei] - st % 65536) % 65536 ei = ei + 1 st = (KM * st + KC) % 268435456 SA[i] = (e[ei] - st % 65536) % 65536 ei = ei + 1 st = (KM * st + KC) % 268435456 SB[i] = (e[ei] - st % 65536) % 65536 ei = ei + 1 st = (KM * st + KC) % 268435456 SC[i] = (e[ei] - st % 65536) % 65536 ei = ei + 1 st = (KM * st + KC) % 268435456 SD[i] = (e[ei] - st % 65536) % 65536 ei = ei + 1 end {cblock} return W, SA, SB, SC, SD, C end end",
			cblock = c_block_dec,
		);
		frags.push((206u16, dec_src.into_bytes()));
		frags.push((207u16, enc_src.into_bytes()));
		// mask + base-94 pack. Per-fragment keystream seed is derived
		// from the wire code ((hseed + wire*hstep) % 2^28) so the
		// decode order (shuffled HQI) is irrelevant.
		let hstep = rng.int(1_048_576, 268_435_455) as u32;
		let alpha = carrier.alphabet;
		let mut hq_lines = String::from("local HQ = {}\n");
		let mut hqi: Vec<String> = Vec::new();
		let mut slots: Vec<i64> = (1..=2000).collect();
		rng.shuffle(&mut slots);
		for (i, (_wire, src)) in frags.iter().enumerate() {
			let mut state = ((hseed as u64 + *_wire as u64 * hstep as u64)
				% 268_435_456) as u64;
			let mut xb: Vec<u8> = src
				.iter()
				.map(|&b| {
					state = (hm as u64 * state + hc as u64) % 268_435_456;
					b.wrapping_add(fold_key(state))
				})
				.collect();
			let blen = xb.len();
			while xb.len() % 4 != 0 {
				xb.push(0);
			}
			let mut digits: Vec<u8> = Vec::with_capacity(xb.len() / 4 * 5);
			for chunk in xb.chunks(4) {
				let mut v = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
				let mut d = [0u8; 5];
				for k in 0..5 {
					d[4 - k] = alpha[(v % 94) as usize];
					v /= 94;
				}
				digits.extend_from_slice(&d);
			}
			let slot = slots[i];
			// clash-free long-string level (printer parity): the FIRST
			// closer `]=*]` in content+closer must land exactly at the
			// content end (guards content ending in `]=*`, which would
			// pull the close forward and strand a stray `]`)
			let digits_s = String::from_utf8(digits).unwrap();
			let mut lvl = 0usize;
			loop {
				let closer = format!("]{}]", "=".repeat(lvl));
				let joined = format!("{}{}", digits_s, closer);
				let first = joined.find(&closer);
				let closer_ok = first == Some(digits_s.len());
				let opener_ok = lvl > 0 || !digits_s.contains("[[");
				if closer_ok && opener_ok {
					break;
				}
				lvl += 1;
			}
			let o = "=".repeat(lvl);
			hq_lines.push_str(&format!(
				"  HQ[{slot}] = [{o}[{d}]{o}]\n",
				slot = slot,
				o = o,
				d = digits_s
			));
			hqi.push(format!("{}, {}, {}", _wire, slot, blen));
		}
		rng.shuffle(&mut hqi);
		// 增量⑩: handler-fragment keystream keys KF-assembled, anchored
		// on #hqi (the fragment-index table declared in the same block).
		// NOTE: hqi is a Vec of "w, s, l" TRIPLET strings that join into
		// a flat table — runtime #hqi = 3 * hqi.len().
		let n_hqi = 3 * hqi.len() as i64;
		let hseed_e = ke.key_expr(hseed as i64, Some(("#hqi", n_hqi)), rng);
		let hstep_e = ke.key_expr(hstep as i64, Some(("#hqi", n_hqi)), rng);
		let hm_e = ke.key_expr(hm as i64, Some(("#hqi", n_hqi)), rng);
		let hc_e = ke.key_expr(hc as i64, Some(("#hqi", n_hqi)), rng);
		manifest_key("HQ_SEED", hseed as u64);
		manifest_key("HQ_STEP", hstep as u64);
		manifest_key("HQ_KM", hm as u64);
		manifest_key("HQ_KC", hc as u64);
		// P4 (防御代码隐藏): the loader/integrity names never appear
		// in the output — each is runtime-built from shuffled char
		// codes (user style), then the nativeness check runs exactly
		// as before (hooked loader -> silent trap, never an oracle).
		// 增量⑮ (引导桩分层): the HQ decode loop itself becomes an
		// encrypted META fragment (HBOOT). The visible boot shrinks to
		// a small meta-decoder with a DIFFERENT codec (additive LCG
		// over a masked byte array, keystream assembled from the KT
		// lookup tables). Static analysts must now: port the
		// meta-decoder -> recover HBOOT -> port the HQ loop -> decode
		// the 44 fragments -> port the parser (⑭). Every layer is a
		// fresh per-build reimplementation.
		let hboot_src = format!(
			"return function(HQ, hqi, AL, BYTE, CHAR, FLR, SUB, LS, KA, KB, KC, KM) local HW = {{}} local BSS local hi = 1 while hi <= #hqi do local w = hqi[hi] local seg = HQ[hqi[hi + 1]] local flen = hqi[hi + 2] hi = hi + 3 local hs = ({hseed} + w * {hstep}) % 268435456 local t = {{}} local ti = 1 local n = #seg for i = 1, n, 5 do local v = 0 v = v * 94 + AL[BYTE(seg, i)] v = v * 94 + AL[BYTE(seg, i + 1)] v = v * 94 + AL[BYTE(seg, i + 2)] v = v * 94 + AL[BYTE(seg, i + 3)] v = v * 94 + AL[BYTE(seg, i + 4)] local b1 = v % 256; v = FLR(v / 256) local b2 = v % 256; v = FLR(v / 256) local b3 = v % 256; v = FLR(v / 256) local b4 = v % 256 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b1 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b2 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b3 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b4 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 end if w == 200 then BSS = SUB(table.concat(t), 1, flen) else HW[w] = LS(SUB(table.concat(t), 1, flen))() end end return HW, BSS end",
			hseed = hseed_e, hstep = hstep_e, hm = hm_e, hc = hc_e,
		);
		// meta keystream: fresh random constants assembled through the
		// same KT lookup machinery (no bare literals; a codec DISTINCT
		// from the base-94 HQ machinery so the analyst gets no free
		// reuse).
		let meta_seed = rng.int(1_048_576, KEY_MOD - 1);
		let meta_m = (rng.int(1_048_577, 33_000_001) | 1) as i64;
		let meta_c = rng.int(1_048_576, KEY_MOD - 1);
		let meta_seed_e = ke.key_expr(meta_seed, None, rng);
		let meta_m_e = ke.key_expr(meta_m, None, rng);
		let meta_c_e = ke.key_expr(meta_c, None, rng);
		// mask HBOOT bytes with the meta keystream (Rust mirror of the
		// visible meta-decoder).
		let hb_bytes = hboot_src.into_bytes();
		let mut hb_masked: Vec<u8> = Vec::with_capacity(hb_bytes.len());
		let mut mst = meta_seed as u64;
		for &b in hb_bytes.iter() {
			mst = (meta_m as u64 * mst + meta_c as u64) % 268_435_456;
			hb_masked.push(b.wrapping_add(fold_key(mst)));
		}
		// emit masked bytes as <=90-entry array chunks (BW-family
		// camouflage; no giant literal array).
		let mut mb_lines = String::new();
		let mut mb_names: Vec<String> = Vec::new();
		for (ci, chunk) in hb_masked.chunks(90).enumerate() {
			let nm = format!("MB{}", ci + 1);
			mb_names.push(nm.clone());
			mb_lines.push_str(&format!(
				"local {} = {{{}}}\n  ",
				nm,
				chunk.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
			));
		}
		let mut mb_gather = String::from("local MB = {}\n");
		for nm in &mb_names {
			mb_gather.push_str(&format!(
				"    for i = 1, #{} do MB[#MB + 1] = {}[i] end\n",
				nm, nm
			));
		}
		let mut boot = String::new();
		let v_ls = "hls";
		let v_dbg = "hdbg";
		let v_inf = "hinf";
		let v_s = "hsarg";
		let v_c = "hcb";
		let v_ts = "hts";
		boot.push_str(&coded_name_tpl(rng, v_ts, "tostring"));
		boot.push_str(&coded_name_tpl(rng, v_ls, "loadstring"));
		boot.push_str(&coded_name_tpl(rng, v_dbg, "debug"));
		boot.push_str(&coded_name_tpl(rng, v_inf, "info"));
		boot.push_str(&coded_name_tpl(rng, v_s, "s"));
		boot.push_str(&coded_name_tpl(rng, v_c, "[C]"));
		// 增量⑯-1 (探针钥匙化): base-31 fold of the clean-environment
		// loadstring source ("[C]"). In a clean run pb == C_FOLD and the
		// extra term vanishes; a hooked loader shifts pb, silently
		// corrupting the meta keystream (HBOOT decodes to garbage ->
		// death by wrong key, independent of the explicit nlok trap).
		let c_fold: i64 = {
			let mut h: i64 = 0;
			for &b in b"[C]".iter() {
				h = (h * 31 + b as i64) % KEY_MOD;
			}
			h
		};
		let probe_mix = rng.int(1_048_576, KEY_MOD - 1);
		let probe_mix_e = ke.key_expr(probe_mix, None, rng);
		let strlit = pool.lit("string");
		// 增量⑰-A: the keystream generation + HBOOT unmask loop become
		// a mini-VM bytecode program (visible dispatcher only). The
		// seed keeps the ⑯-1 probe-keyed form, fed as MM[1].
		//
		// 增量⑱ (选项B路线一 — 输入绑定): when bound, the seed gains a
		// term `(ak - AK) * mix` where ak = fold31(first vararg) is
		// computed at boot and AK = fold31(activation) is assembled
		// from the key tables (no literal). Correct activation -> the
		// term vanishes and HBOOT decodes; wrong/missing input shifts
		// the meta keystream -> HBOOT decodes to garbage -> loadstring
		// yields nil -> death before ANY bytecode surfaces. The whole
		// decode pipeline (HBOOT -> HQ fragments -> parser) sits
		// behind this one gate; the compile-time masking mirror is
		// untouched (its seed is the constant meta_seed).
		let mut ak_fold: i64 = 0;
		let mut ak2_fold: i64 = 0;
		if let Some(key) = bind_key {
			// 增量⑳: two independent nonlinear factors (mirror in the gate)
			ak_fold = (activation_hash(key.as_bytes(), 0x811c_9dc5, 7, 31)
				% 268_435_456) as i64;
			ak2_fold = (activation_hash(key.as_bytes(), 0x0100_0193, 13, 37)
				% 268_435_456) as i64;
			assert!(ak_fold != 0, "activation fold collapsed to 0 (== no-input fold)");
		}
		let (ak_gate, meta_seed_full) = if bind_key.is_some() {
			let ak_e = ke.key_expr(ak_fold, Some(("#hqi", n_hqi)), rng);
			let ak2_e = ke.key_expr(ak2_fold, Some(("#hqi", n_hqi)), rng);
			let bind_mix = rng.int(1_048_576, KEY_MOD - 1);
			let bind_mix_e = ke.key_expr(bind_mix, Some(("#hqi", n_hqi)), rng);
			let bind_mix2 = rng.int(1_048_576, KEY_MOD - 1);
			let bind_mix2_e = ke.key_expr(bind_mix2, Some(("#hqi", n_hqi)), rng);
			manifest_key("BIND_AK", ak_fold as u64);
			manifest_key("BIND_AK2", ak2_fold as u64);
			manifest_key("BIND_MIX", bind_mix as u64);
			manifest_key("BIND_MIX2", bind_mix2 as u64);
			let dbg = if std::env::var("LURAPH_BIND_DBG").is_ok() {
				"print('DBGAK', TYP(akv), TSTR(akv), ak1, ak2)\n    "
			} else {
				""
			};
			// 增量⑳: nonlinear two-factor fold (bit32). Seeds are emitted
			// through obf_num (no recognizable FNV constant in output);
			// the gate lives in the visible boot but its TARGET (AK1/AK2)
			// stays assembled in the key tables.
			let seed1_e = obf_num(0x811c_9dc5, rng);
			let seed2_e = obf_num(0x0100_0193, rng);
			let gate = format!(
				"local akv = ...\n    local ak1 = {seed1}\n    local ak2 = {seed2}\n    if TYP(akv) == {strlit} then\n      for aki = 1, #akv do\n        local b = BYTE(akv, aki)\n        ak1 = bit32.bxor(ak1, b)\n        ak1 = bit32.lrotate(ak1, 7)\n        ak1 = bit32.bxor(ak1, bit32.band(ak1 * 31 + b, 4294967295))\n        ak2 = bit32.bxor(ak2, b)\n        ak2 = bit32.lrotate(ak2, 13)\n        ak2 = bit32.bxor(ak2, bit32.band(ak2 * 37 + b, 4294967295))\n      end\n      ak1 = ak1 % 268435456\n      ak2 = ak2 % 268435456\n    end\n    {dbg}    ",
				seed1 = seed1_e,
				seed2 = seed2_e,
				strlit = strlit,
				dbg = dbg,
			);
			let seed = format!(
				"({} + (pb - {}) * {} + (ak1 - {}) * {} + (ak2 - {}) * {}) % 268435456",
				meta_seed_e, c_fold, probe_mix_e, ak_e, bind_mix_e, ak2_e, bind_mix2_e
			);
			(gate, seed)
		} else {
			(
				String::new(),
				format!(
					"({} + (pb - {}) * {}) % 268435456",
					meta_seed_e, c_fold, probe_mix_e
				),
			)
		};
		let metavm = emit_metavm(&meta_seed_full, &meta_m_e, &meta_c_e, hb_masked.len(), rng);
		// 增量⑲: build-time-shuffled continuation wiring (which CT slot
		// holds which encrypted re-entry fragment is per-build random).
		let mut cont_wires: Vec<u16> = (301..305).collect();
		rng.shuffle(&mut cont_wires);
		let ct_fill: String = (0..4)
			.map(|i| format!("CT[{}] = HW[{}]\n    ", i + 1, cont_wires[i]))
			.collect();
		// call executors: fixed callx->wire mapping (the chain phases
		// stash E.callx = 1..4), emit order shuffled (cosmetic)
		let mut cx_order: Vec<usize> = (0..4).collect();
		rng.shuffle(&mut cx_order);
		let cx_fill: String = cx_order
			.iter()
			.map(|&i| format!("CX[{}] = HW[{}]\n    ", i + 1, 305 + i))
			.collect();
		let build = format!(
			r#"{}  {}local HW = {{}}
  local BSS
  local AV = 0
  local HW2 = {{}}
  local CT = {{}}
  local CX = {{}}
  local DDEC
  local MPF
  do
    {}local LS = GFE(0)[{v_ls}]
    local TSTR = GFE(0)[{v_ts}]
    local DBG = GFE(0)[{v_dbg}]
    local INF = DBG and DBG[{v_inf}]
    local nlok = false
    local pb = 0
    do
      local ok, sr = PCAL(function() return INF(LS, {v_s}) end)
      if ok and sr == {v_c} then nlok = true end
      if ok and TYP(sr) == {strlit} then
        for i = 1, #sr do pb = (pb * 31 + BYTE(sr, i)) % 268435456 end
      end
    end
    if not nlok then while true do end end
    {ak_gate}local hqi = {{{}}}
    {}    {}
    HW, BSS = LS(table.concat(MH))()(HQ, hqi, AL, BYTE, CHAR, FLR, SUB, LS, KA, KB, KC, KM)
    HQ = nil; hqi = nil
    {ct_fill}{cx_fill}DDEC = HW[206]({pseed}, {pstep}, {pkm}, {pkc})
    local ENCF = HW[207]
    do
      local avt = {{}}
      local ats = TSTR(avt)
      for i = 1, #ats do AV = (AV * 31 + BYTE(ats, i)) % 268435456 end
    end
    for w, f in pairs(HW) do if w < 256 then HW2[(w + AV) % 256] = f end end
    HW = nil
    PF = LS(BSS)()(BYTE, CHAR, FLR, SUB, AL, TK, decarrier, r16, CKM, CKC, BKM, BKC, BSEED, BSTEP)(FN)
    BSS = nil
    {zw_lines}MPF = ENCF(PF, FLR, TYP, BYTE, TSTR, KA, KB, KC, KM)
    PF = nil
  end"#,
			hq_lines, mb_lines, boot, hqi.join(", "), mb_gather, metavm,
			ak_gate = ak_gate,
			strlit = strlit,
			v_ls = v_ls, v_ts = v_ts, v_dbg = v_dbg, v_inf = v_inf,
			v_s = v_s, v_c = v_c,
			zw_lines = v15_zw,
			pseed = pseed_e, pstep = pstep_e, pkm = pkm_e, pkc = pkc_e,
		);
		hfrag = build;
		String::new()
	} else {
		hfrag = String::new();
		String::new()
	};
	let (fetch, branches) = if v15 {
		// 增量⑲ (CPS 去中心化): the old fetch string is dead — run() is
		// now the trampoline loop (see run_loop below); every dispatch
		// step lives inside the encrypted handler epilogues / the four
		// encrypted continuation fragments.
		(String::new(), String::new())
	} else {
		(fetch, branches)
	};

	// runtime helpers with pool-routed literals (resolve_call's type/
	// meta names, callcap's '#' vararg selector)
	// 增量⑲: v15 callcap captures multi-returns through table.pack (one
	// C frame, pops immediately) instead of the `w` wrapper closure (a
	// persistent Lua frame for the whole nested call — recursion frame
	// budget). Legacy (Lua 5.1, no table.pack) keeps `w`.
	let callcap_src = if v15 {
		format!(
			"local TP = GFE(0)[{table}][{pack}]
  local function callcap(f, args, nargs)
    local r = TP(f(U(args, 1, nargs)))
    return r, r.n
  end",
			table = pool.lit("table"),
			pack = pool.lit("pack"),
		)
	} else {
		format!(
			"local function callcap(f, args, nargs)
    local w = function(...)
      local t = {{ ... }}
      return t, SEL({hash}, ...)
    end
    return w(f(U(args, 1, nargs)))
  end",
			hash = pool.lit("#"),
		)
	};
	let rt_helpers = format!(
		"local function mget(x, k)
    local mt = GMT(x)
    if mt then return mt[k] end
    return nil
  end
  local function resolve_call(f)
    if TYP(f) == {function} then return f, false end
    local mt = GMT(f)
    local cc = mt and mt[{call}]
    if TYP(cc) == {function} then return cc, true end
    if TYP(cc) == {table} then
      local cf = cc[f]
      if TYP(cf) == {function} then return cf, false end
    end
    ERR({callmsg} .. TYP(f) .. {value}, 0)
  end
  {callcap}",
		function = pool.lit("function"),
		call = pool.lit("__call"),
		table = pool.lit("table"),
		callmsg = pool.lit("attempt to call a "),
		value = pool.lit(" value"),
		callcap = callcap_src,
	);
	let ms_block = if v15 { pool.boot_block() } else { String::new() };

	// 增量⑩: the key-fragment table + all fragment writes land at the
	// very top of the VM body (before the first key use in oc_boot).
	let kf_block = ke.block(rng);
	// 增量⑱ (输入绑定): bound output gives the VM a vararg tail — the
	// entry closure forwards (activation, data...) after the carriers;
	// the boot gate folds the first vararg, the program receives the
	// rest (see entry_tail below). Unbound output keeps the exact
	// historical signature.
	let vm_params = if bind_key.is_some() {
		format!("{}, ...", params)
	} else {
		params.clone()
	};
	// ㉒: v15 entry runs the encoded ROOT prototype (MPF) — PF itself
	// was destroyed at boot. Legacy keeps the historical PF[#FN] form.
	let entry_pf = if v15 { "MPF" } else { "PF[#FN]" };
	let entry_tail = if bind_key.is_some() {
		format!(
			"local _bt = {{ ... }}\n  local vargs = {{}}\n  for _bi = 2, #_bt do\n    vargs[_bi - 1] = _bt[_bi]\n  end\n  local V2 = {{}}\n  return run({}, V2, {{}}, vargs, #vargs)",
			entry_pf
		)
	} else {
		format!("local vargs = {{}}\n  local V2 = {{}}\n  return run({}, V2, {{}}, vargs, 0)", entry_pf)
	};
	// 增量⑲: v15 run() = chain entry (E.pc lives in the E constructor;
	// the body is ONE chain epilogue tail-calling the first handler).
	// Legacy keeps the pc local + the fetch/dispatch while loop.
	// 增量⑲ (CPS 去中心化, Luau-可行形态): Luau 无正确尾调用
	// （devforum 3307934 官方确认移除），纯尾链每指令积 1 帧、~16k 帧
	// 溢栈，跑不了任意循环。故采用**预算有界尾链 + 蹦床**：每条链最多
	// 走 ~CHAIN_BUDGET 条指令（预算耗尽即空返回、整链解开），蹦床循环
	// 重置预算、轮转 4 个加密重入碎片之一从 E.pc 续跑；Call 家族拆两相，
	// 实际调用在蹦床浅层执行（深递归帧剖面回到旧设计量级）。分派逻辑
	// 全在 43 个加密 handler + 4 个重入碎片 + 4 个执行器碎片里，可见
	// 蹦床无操作码语义（R006 壁垒②）。run() 本体瘦成蹦床（帧开销留给
	// 用户递归：E 构造移入 newE，其帧随返即销）。
	let (run_head, newe_fwd, newe_decl, pc_decl, run_loop) = if v15 {
		// newE's body builds E (references makefn/mget/... — declared
		// above it), while the makefn closures call newE: forward-declare
		// the local, define it after makefn.
		let newe = format!(
			"newE = function(pf, V, ups, vargs, vargc)\n    {}\n    local W, SA, SB, SC, SD, C = DDEC(pf, FLR, CHAR, UNP, TONUM)\n    local S = pf.S\n    local O = {{}}\n    return {}\n  end\n  ",
			run_unpack, e_table
		);
		// the unpack/SoA lines now live inside newE — keep them out of
		// run's frame (recursion frame budget)
		run_unpack = String::new();
		run_soa = String::new();
		(
			String::from("local E = newE(pf, V, ups, vargs, vargc)\n    "),
			String::from("local newE\n  "),
			newe,
			String::new(),
			trampoline_src.clone().unwrap(),
		)
	} else {
		(
			String::new(),
			String::new(),
			String::new(),
			String::from("local pc = 1\n    "),
			format!("while true do\n      {}\n      {}\n    end", fetch, branches),
		)
	};
	format!(
		r#"local VM = function({vm_params})
  {kf_block}{oc_table}
  local FN = {{{params}}}
  local PF = {{}}
  {p_fill}  {prim_unpack}
  {ms_block}{helpers}
  {ck_consts}{parse_fn}
  {decode_seg}
{hfrag}{v15_selfmod}
  local G = GFE(0)
  local U = UNP
  local FLOOR = FLR
  local _probe = SMT({{}}, {{ __len = function() return 99 end }})
  local HAS_LEN_META = (_probe == nil) or false
  do
    local okp, vp = PCAL(function() return #_probe end)
    HAS_LEN_META = okp and vp == 99
  end
  {rt_helpers}
  {hub_decl}
  {newe_fwd}local run
  {makefn_decl}
  {newe_decl}run = function(pf, V, ups, vargs, vargc)
    {run_head}{run_unpack}
    {run_soa}
    {pc_decl}{o_decl}{ln_decl}{handler_defs}{run_loop}
  end
  {entry_tail}
end
"#,
	vm_params = vm_params,
	entry_tail = entry_tail,
	newe_fwd = newe_fwd,
	pc_decl = pc_decl,
	run_loop = run_loop,
	params = params,
		kf_block = kf_block,
		oc_table = oc_table,
		p_fill = p_fill,
		prim_unpack = prim_unpack,
		helpers = helpers,
		hub_decl = hub_decl,
		run_unpack = run_unpack,
		run_soa = run_soa,
		handler_defs = handler_defs,
		hfrag = hfrag,
		newe_decl = newe_decl,
		run_head = run_head,
		ln_decl = if v15 {
			String::new()
		} else {
			"local lastn = 0\n    local lastbase = 0\n    ".to_string()
		},
		v15_selfmod = v15_selfmod,
		decode_seg = decode_seg,
		ck_consts = ck_consts,
		parse_fn = parse_fn_vis,
		makefn_decl = makefn_decl,
		rt_helpers = rt_helpers,
		ms_block = ms_block,
		o_decl = String::new(),
	)
}
