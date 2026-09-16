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
fn emit_metavm(
	seed_e: &str,
	m_e: &str,
	c_e: &str,
	mb_count: usize,
	rng: &mut Rng,
	mpm_seed: i64,
	mpm_m: i64,
	mpm_c: i64,
	mpm_seed_e: &str,
	mpm_m_e: &str,
	mpm_c_e: &str,
) -> String {
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
	// ㉛ (R014 回合 — P1-3): R014 破解的起点 = 明文程序字直接 Python
	// 静态模拟（173,778 步、0.04s）。程序字改为加性 LCG 掩码落盘、引导
	// 期即解——静态侦察看到的是一串无结构大整数，模拟前必须先移植解
	// 掩码环（且钥匙每构建重抽）。掩码 = 每字 +(4096 + ks % 2.64e8)：
	// 下界 4096 保证负偏移字（回跳 ~-300）掩后仍为正，解掩是纯减法。
	let mut mks = mpm_seed;
	for w in p.iter_mut() {
		mks = (mpm_m * mks + mpm_c) % KEY_MOD;
		*w += 4096 + mks % 264_000_000;
	}
	// ---- emit -------------------------------------------------------
	let mut mp = String::from("local MP = {");
	mp.push_str(
		&p.iter()
			.map(|v| v.to_string())
			.collect::<Vec<_>>()
			.join(", "),
	);
	mp.push_str("}\n");
	mp.push_str(&format!(
		"    do\n      local mps, mpk, mpq = {}, {}, {}\n      for mpi = 1, #MP do mps = (mpk * mps + mpq) % 268435456 MP[mpi] = MP[mpi] - (4096 + mps % 264000000) end\n    end\n",
		mpm_seed_e, mpm_m_e, mpm_c_e
	));
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
/// ㉘-A (R011 回合 — 碎片源码净化): 攻击方解出碎片后拿到满手可读名
/// (parse/decarrier/nregs/upsrc/makefn/bdec/CDEC) 和**设计意图注释**
/// (「自研混淆器 + 迭代版本 + trampoline 原理」直接送情报)。净化三步:
/// ① 剥全部 `--` 注释（字符串感知）；② 全部非保留标识符（局部/参数/
/// **字段名**）按构建随机改名（跨碎片一致映射）；③ 空白折叠成单行。
/// 保留集 = Lua 关键字 + 运行时固定全局名（pairs/table/string/...）。
/// ㉘-A: 碎片净化专用短名生成器。碎片内标识符集有限（百级），2–4 字符
/// 名空间足够且碰撞可控；相比通用 gen_name（30% 概率出 9–15 字符长名）
/// 显著压缩碎片体积（净化名会进加密 blob，长度直接计入产物）。
fn sanitize_name(
	rng: &mut Rng,
	reserved: &std::collections::HashSet<String>,
	used: &std::collections::HashSet<String>,
) -> String {
	const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";
	loop {
		let len = match rng.int(0, 99) {
			0..=59 => 2,
			60..=89 => 3,
			_ => 4,
		};
		let mut name = String::with_capacity(len);
		// 首字符不能是数字; 直接用字母/下划线开头
		for k in 0..len {
			let pool = if k == 0 { &CHARS[..] } else { CHARS };
			name.push(pool[rng.int(0, pool.len() as i64 - 1) as usize] as char);
		}
		if !reserved.contains(&name) && !used.contains(&name) {
			return name;
		}
	}
}

// ㉚ (B-2 源码自校验哈希链): 双车道非线性字节哈希，Lua 5.1 / Luau
// 逐位一致（纯 + * % 算术，全程 < 2^52 无精度损失）。HBOOT 在运行时
// 对「自身源码 + 全部 HQ 碎片解码态源码」走同一条链；构建期 Rust 用
// 本镜像算出期望终值，把偏差归零常量揉进运行时钥匙派生——哈希不再
// 是可比对的明文常量，而是**钥匙本身**：改动任何一层源码 ⇒ 终值偏移
// ⇒ 块钥匙流静默错位（程序照跑、解密内容全错），攻击者除预映像攻击
// 无路可走。模数取 2^26 附近两个素数（乘法积 < 2^51 精确）。
pub const SH_M1: u64 = 67_108_859; // 2^26 - 5 (prime)
pub const SH_M2: u64 = 67_108_819; // 2^26 - 45 (prime)

fn sh_step(h1: u64, h2: u64, b: u8, k1: u64, k2: u64) -> (u64, u64) {
	let n1 = (h1.wrapping_mul(k1) + b as u64 * (h2 % 97 + 1)) % SH_M1;
	let n2 = (h2.wrapping_mul(k2) + b as u64 * (n1 % 89 + 1)) % SH_M2;
	(n1, n2)
}

fn sh_bytes(bytes: &[u8], mut h1: u64, mut h2: u64, k1: u64, k2: u64) -> (u64, u64) {
	for &b in bytes {
		let (a, c) = sh_step(h1, h2, b, k1, k2);
		h1 = a;
		h2 = c;
	}
	(h1, h2)
}

/// ㉟ (自研压缩 — 碎片增量链): 每片对「此前全部已解码源码的拼接字典」
/// 做贪心 LZ 增量编码。HBOOT 引导本来就在 loadstring 之前握着所有解码态
/// 源码——解码链即压缩链：操作流只有两种，字面段 (00 + 长度 + 字节) 与
/// 字典引用 (01 + 1 基偏移 + 长度)，全 2 字节小端，字典 ≤ ~50KB 恰好
/// 65535 封顶。Rust 编码 / HBOOT 解码，逐字节镜像。实测语料碎片
/// 49.8KB → 24.6KB（49%），且与解码顺序无关（增长字典吸收顺序差异）。
fn lz_chain_encode(cur: &[u8], dict: &[u8]) -> Vec<u8> {
	let min_match: usize = 8;
	// 4-gram 索引（字典上）
	let mut idx: std::collections::HashMap<[u8; 4], Vec<usize>> =
		std::collections::HashMap::new();
	if dict.len() >= 4 {
		for i in 0..dict.len() - 3 {
			let g: [u8; 4] = [dict[i], dict[i + 1], dict[i + 2], dict[i + 3]];
			idx.entry(g).or_default().push(i);
		}
	}
	let mut out: Vec<u8> = Vec::with_capacity(cur.len() / 2 + 16);
	out.extend_from_slice(&(cur.len() as u16).to_le_bytes());
	let mut lit: Vec<u8> = Vec::new();
	fn flush_lit(out: &mut Vec<u8>, lit: &mut Vec<u8>) {
		let mut rest = &lit[..];
		while !rest.is_empty() {
			let n = rest.len().min(65535);
			out.push(0x00);
			out.extend_from_slice(&(n as u16).to_le_bytes());
			out.extend_from_slice(&rest[..n]);
			rest = &rest[n..];
		}
		lit.clear();
	}
	let mut i: usize = 0;
	while i < cur.len() {
		let mut best_len: usize = 0;
		let mut best_off: usize = 0;
		if i + 4 <= cur.len() {
			let g: [u8; 4] = [cur[i], cur[i + 1], cur[i + 2], cur[i + 3]];
			if let Some(cands) = idx.get(&g) {
				for &pos in cands.iter().rev().take(64) {
					let mut l = 0;
					while i + l < cur.len()
						&& pos + l < dict.len()
						&& cur[i + l] == dict[pos + l]
					{
						l += 1;
					}
					if l > best_len {
						best_len = l;
						best_off = pos + 1; // 1 基
					}
				}
			}
		}
		if best_len >= min_match && best_off <= 65535 && best_len <= 65535 {
			flush_lit(&mut out, &mut lit);
			out.push(0x01);
			out.extend_from_slice(&(best_off as u16).to_le_bytes());
			out.extend_from_slice(&(best_len as u16).to_le_bytes());
			i += best_len;
		} else {
			lit.push(cur[i]);
			i += 1;
		}
	}
	flush_lit(&mut out, &mut lit);
	out
}

/// ㉟: 解码镜像（与 HBOOT 重建器逐字节一致）——构建期往返自检用。
#[cfg(debug_assertions)]
fn lz_chain_decode_check(ser: &[u8], dict: &[u8]) -> Vec<u8> {
	let ulen = ser[0] as usize + (ser[1] as usize) * 256;
	let mut out: Vec<u8> = Vec::new();
	let mut pi: usize = 2; // 0 基，跳过 ulen 前缀
	while pi < ser.len() {
		if ser[pi] == 0 {
			let n = ser[pi + 1] as usize + (ser[pi + 2] as usize) * 256;
			out.extend_from_slice(&ser[pi + 3..pi + 3 + n]);
			pi += 3 + n;
		} else {
			let off = ser[pi + 1] as usize + (ser[pi + 2] as usize) * 256;
			let n = ser[pi + 3] as usize + (ser[pi + 4] as usize) * 256;
			out.extend_from_slice(&dict[off - 1..off - 1 + n]);
			pi += 5;
		}
	}
	out.truncate(ulen);
	out
}

/// ㉚: 碎片自由填充尾注（>= 8B，实际 14–22B 随机）——回填搜索空间
/// + 每构建长度熵。尾注释对 loadstring 无副作用，且进哈希链。
fn frag_pad(src: &mut Vec<u8>, rng: &mut Rng) {
	const PADCHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyzABDEFGHJKLMNPQRSTUVWXYZ23456789";
	src.extend_from_slice(b"--");
	let pn = rng.int(12, 20);
	for _ in 0..pn {
		src.push(PADCHARS[rng.int(0, PADCHARS.len() as i64 - 1) as usize]);
	}
}

fn sanitize_frag(
	src: &str,
	rng: &mut Rng,
	map: &mut std::collections::HashMap<String, String>,
	reserved: &std::collections::HashSet<String>,
	used: &mut std::collections::HashSet<String>,
) -> String {
	// 保留字全局名集合（其后的 `.字段` 是标准库方法，禁止改名）
	static GLOBALS: &[&str] = &[
		"table", "string", "math", "bit32", "os", "debug", "buffer",
		"task", "coroutine", "utf8", "_G", "game", "workspace", "Instance",
		"Vector3", "Vector2", "Enum",
	];
	let is_global = |w: &str| GLOBALS.contains(&w);
	// ㉘-A 关键: 运行时 C 函数创建的字段名禁改（table.pack 的 .n）
	static RUNTIME_FIELDS: &[&str] = &["n"];
	let b = src.as_bytes();
	let mut out = String::with_capacity(src.len());
	let mut i = 0usize;
	let mut quote: Option<u8> = None;
	let mut escaped = false;
	let mut pending_space = false;
	let mut prev_word: Option<String> = None; // 最近标识符(原文)
	let mut after_dot = false; // 上个有效字符是 `.` 或 `:`
	while i < b.len() {
		let c = b[i];
		if let Some(q) = quote {
			out.push(c as char);
			if escaped {
				escaped = false;
			} else if c == b'\\' {
				escaped = true;
			} else if c == q {
				quote = None;
			}
			i += 1;
			continue;
		}
		match c {
			b'\'' | b'"' => {
				if pending_space {
					out.push(' ');
					pending_space = false;
				}
				quote = Some(c);
				out.push(c as char);
				prev_word = None;
				after_dot = false;
				i += 1;
			}
			b'-' if i + 1 < b.len() && b[i + 1] == b'-' => {
				while i < b.len() && b[i] != b'\n' {
					i += 1;
				}
				pending_space = true;
			}
			b'\n' | b'\r' | b' ' | b'\t' => {
				// 空白在 Lua 里非语句分隔符，折叠即可（误插 `;` 会在
				// function(...)/then/do 后产生非法语法）
				pending_space = true;
				i += 1;
			}
			b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
				let start = i;
				while i < b.len()
					&& (b[i].is_ascii_alphanumeric() || b[i] == b'_')
				{
					i += 1;
				}
				let word = &src[start..i];
				if pending_space {
					out.push(' ');
					pending_space = false;
				}
				// ㉘-A 关键: 保留字全局名后的字段 = 标准库方法
				// (table.concat 等)，绝不改名；其余标识符（局部/参数/
				// 自有字段名）按构建映射改名，跨碎片一致。
				let keep_field = RUNTIME_FIELDS.contains(&word)
					|| (after_dot
						&& prev_word.as_deref().map(is_global).unwrap_or(false));
				if reserved.contains(word) || keep_field {
					out.push_str(word);
				} else {
					if !map.contains_key(word) {
						// ㉘-A 关键: 新名字必须与保留字和所有已生成名字
						// 都不碰撞，否则两个不同标识符会合并成同一名字。
						let name = sanitize_name(rng, reserved, used);
						used.insert(name.clone());
						map.insert(word.to_string(), name);
					}
					out.push_str(&map[word]);
				}
				prev_word = Some(word.to_string());
				after_dot = false;
			}
			_ => {
				if pending_space {
					out.push(' ');
					pending_space = false;
				}
				out.push(c as char);
				after_dot = c == b'.' || c == b':';
				if !after_dot {
					prev_word = None;
				}
				i += 1;
			}
		}
	}
	out
}


fn chain_epilogue(rng: &mut Rng, wk: &[String; 4]) -> String {
	// B-3: fetch reads the decoded BLOCK WINDOW (E.ww/E.w0..w3) instead
	// of whole-stream tables. Crossing the window bounds (jump target /
	// fall-through merge) triggers the per-block decoder E.bdec — block
	// keys carry the session salt, so decoding only ever happens along a
	// live execution path. Same five polymorphic shapes as ⑲.
	const NP_POOL: [&str; 6] = ["np", "gz", "hz", "kz", "pz", "wz"];
	const OC_POOL: [&str; 6] = ["oc", "jz", "qz", "xz", "yz", "nz"];
	const H_POOL: [&str; 6] = ["hx", "fx", "zx", "vx", "mx", "jx"];
	const B_POOL: [&str; 6] = ["bi", "bz", "dz", "ez", "fz", "iz"];
	let np = NP_POOL[rng.int(0, 5) as usize];
	let oc = OC_POOL[rng.int(0, 5) as usize];
	let h = H_POOL[rng.int(0, 5) as usize];
	let bi = B_POOL[rng.int(0, 5) as usize];
	let budget = if rng.int(0, 1) == 0 {
		"E.b = E.b - 1; if E.b == 0 then return end; "
	} else {
		"E.b = E.b - 1; if E.b <= 0 then return end; "
	};
	let trans = format!(
		"if {np} < E.bp or {np} > E.be then E.bdec(E, E.bm[{np}]) end; local {bi} = {np} - E.bp + 1",
		np = np,
		bi = bi
	);
	let args = format!(
		"{wa}[{bi}], {wb}[{bi}], {wc}[{bi}], {wd}[{bi}]",
		wa = wk[0],
		wb = wk[1],
		wc = wk[2],
		wd = wk[3],
		bi = bi
	);
	match rng.int(0, 4) {
		0 => format!(
			"{budget}local {np} = E.pc; {trans}; local {oc} = E.ww[{bi}]; E.pc = {np} + 1; return HW2[({oc} + AV) % 256](E, {args})",
			budget = budget, np = np, trans = trans, oc = oc, bi = bi, args = args
		),
		1 => format!(
			"{budget}local {np} = E.pc; E.pc = {np} + 1; {trans}; local {oc} = E.ww[{bi}]; local {h} = HW2[({oc} + AV) % 256]; return {h}(E, {args})",
			budget = budget, np = np, trans = trans, oc = oc, h = h, bi = bi, args = args
		),
		2 => format!(
			"{budget}local {np} = E.pc; {trans}; E.pc = {np} + 1; return HW2[(E.ww[{bi}] + AV) % 256](E, {args})",
			budget = budget, np = np, trans = trans, bi = bi, args = args
		),
		3 => format!(
			"{budget}local {np} = E.pc; {trans}; local {h} = HW2[(E.ww[{bi}] + AV) % 256]; E.pc = {np} + 1; return {h}(E, {args})",
			budget = budget, np = np, trans = trans, h = h, bi = bi, args = args
		),
		_ => format!(
			"{budget}local {np} = E.pc; {trans}; local {oc} = E.ww[{bi}]; local oa, ob, od2, oe = {args}; E.pc = {np} + 1; return HW2[({oc} + AV) % 256](E, oa, ob, od2, oe)",
			budget = budget, np = np, trans = trans, oc = oc, bi = bi, args = args
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
	// B-3: per-prototype basic-block leader PCs (1-based), parallel to
	// the FN carrier order (children 0..n-2, main last). v15 only.
	block_starts: &[Vec<u16>],
	// ㉔ (R008 回合 — 环境值绑定): Some("roblox") folds TARGET-RUNTIME
	// VALUES (Vector3/Vector2 component math, per-build random args)
	// into the boot keystream — a mock stub that ignores constructor
	// args contaminates the key. v15 only.
	bind_env: Option<&str>,
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
	// ㉓ (R008 回合): the Nop-alias self-modification writes move INTO
	// the BSS encrypted fragment (applied right after parse, before the
	// in-fragment re-encode) — the decoded streams never surface in
	// visible code. The visible F14/F27 ZW shape is given up (security
	// first, P3c precedent).
	let v15_selfmod = if v15 {
		let mut sm = String::new();
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
		// ㉗: 续行/执行器不再经可见表索引（CT[i]/CX[i] 是文本手术包装
		// 点）——蹦床从离散上游值选取，入密后它们是运行时碎片的参数。
		// ㉙-2 (反 trace 计时守卫): 自适应基线——前 4 个 128 循环窗取
		// 最小耗时为基线（最不受 GC 污染），其后任一窗耗时 > 基线 100
		// 倍且连续 2 窗 → 判定逐指令插桩（攻击方自述拖慢几个数量级），
		// 污染 SLT → 后续块解码错钥静默死亡。自适应 → 设备无关，不误伤
		// 慢机；100 倍 + 连续 2 窗 → 不误伤偶发 GC 停顿。
		Some(format!(
			"local ti = 0\n    local _tw = CLK and CLK() or 0\n    local _wcount = 0\n    local _minbase = nil\n    local _tslow = 0\n    while true do\n      E.b = {}\n      if E.callx > 0 then\n        local cx = E.callx == 1 and XE1 or E.callx == 2 and XE2 or E.callx == 3 and XE3 or XE4\n        E.callx = 0\n        cx(E)\n      else\n        local cs = ti % 4\n        local cf = cs == 0 and XC1 or cs == 1 and XC2 or cs == 2 and XC3 or XC4\n        cf(E)\n        ti = ti + 1\n      end\n      if CLK and ti % 128 == 0 then local _tn = CLK() local _dt = _tn - _tw _tw = _tn _wcount = _wcount + 1 if _wcount <= 4 then if not _minbase or _dt < _minbase then _minbase = _dt end else if _minbase and _minbase > 0 and _dt > _minbase * 100 then _tslow = _tslow + 1 else _tslow = 0 end if _tslow >= 2 then SLT = SLT + 7777777 SLTC = SLTC + 7777777 end end end\n      if E.done then return U(E.out, 1, E.total) end\n    end",
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
	// B-3: operand k rides in window E.w{operand_stream[k]} (stream-slot
	// order); the opcode wire rides in E.ww.
	let wk = [
		format!("E.w{}", operand_stream[0]),
		format!("E.w{}", operand_stream[1]),
		format!("E.w{}", operand_stream[2]),
		format!("E.w{}", operand_stream[3]),
	];
	let env_names = [
		"V", "C", "S", "O", "G", "vargs", "vargc", "ups", "makefn", "mget",
		"resolve_call", "callcap", "HAS_LEN_META", "CHAR", "FLOOR", "ERR",
		"TYP", "GMT", "RGET", "RSET", "U", "MS",
		"AV", "HW2",
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
					"return function(E) local a, b, c, d = E.oa, E.ob, E.oc2, E.od; local fn, {}, nargs = E.cf, E.ca, E.cn; local V, S, O, U, AV, HW2 = E[1], E[3], E[4], E[21], E[23], E[24]; {}{} {} end",
					aname,
					exec_pre,
					post_s,
					chain_epilogue(rng, &wk),
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
				body.push_str(&chain_epilogue(rng, &wk));
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
				chain_epilogue(rng, &wk);
			let src = format!(
				"return function(E) local AV, HW2 = E[23], E[24] {} end",
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
		// ㉓ (R008 回合 — 解码窗口入密): R008 实测破解路径 = 对可见
		// boot 做文本手术，在「解码态原型表落地可见局部」的一点插入转储
		// （或干脆抹掉 ENC 调用），㉒ 的内存侧防护对静态手术无效。根治：
		// 解码态原型**从不出现在可见代码里**——BSS 碎片(解析器)直接内联
		// 自改写 + ENC 重编码，只把**编码态树根**递回可见层。可见 boot
		// 从此只见密文；攻击者要拿解码态必须先破 HQ 加密（碎片钥匙→
		// HBOOT→HQ 循环→BSS 碎片→碎片内移植解析器+编码器，每构建重建）。
		// 原型编码钥匙（每原型独立 LCG 流，种子 = pseed + i*pstep）。
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
		// B-3: per-block keystream stride + session salt wiring. Block
		// seed = (pseed + proto*pstep + block_start*pblock + SALT);
		// SALT is a BOOT-RANDOM fold (fresh table address) — the
		// at-rest file can never predict it, so no offline decoder
		// exists (the decoder needs a value that only a live run
		// materializes).
		let pblock = rng.int(1_048_576, 268_435_455) as i64;
		let pblock_e = ke.key_expr(pblock, None, rng);
		manifest_key("PF_BLOCK", pblock as u64);
		// operand-b stream (carries the Closure child index) + the
		// Closure wire byte — both baked into the in-BSS encode scan
		let closure_wire = map.to_wire
			[OP_NAMES.iter().position(|n| *n == "Closure").unwrap()];
		// 常量编码块（类型折叠进掩码类型槽：0=nil 1=整数加性 2=字符串
		// 3=true 4=false 5=tostring 往返；字节逐个掩码）
		let c_block_enc = if consts_safe {
			String::from(
				"local Ct = pf.C local m = 0 for j in pairs(Ct) do if j > m then m = j end end local ct, cn, csb, csl = {}, {}, {}, {} local si = 1 for j = 1, m do st = (KM * st + KC) % 268435456 local v = Ct[j] local tv = TYP(v) if tv == \"number\" then if v % 1 == 0 and v > -1125899906842624 and v < 1125899906842624 then ct[j] = (1 + st) % 65536 cn[j] = v + st else local ns = TOSTR(v) ct[j] = (5 + st) % 65536 csl[j] = #ns for cb = 1, #ns do st = (KM * st + KC) % 268435456 csb[si] = (BYTE(ns, cb) + st % 256) % 256 si = si + 1 end end elseif tv == \"string\" then ct[j] = (2 + st) % 65536 csl[j] = #v for cb = 1, #v do st = (KM * st + KC) % 268435456 csb[si] = (BYTE(v, cb) + st % 256) % 256 si = si + 1 end elseif tv == \"boolean\" then if v then ct[j] = (3 + st) % 65536 else ct[j] = (4 + st) % 65536 end else ct[j] = st % 65536 end end",
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
		let os_list = operand_sums
			.iter()
			.map(|s| obf_num(*s, rng))
			.collect::<Vec<_>>()
			.join(", ");
		// ㉓: Nop 别名自改写从可见 boot 挪进 BSS 碎片（编码前执行）。
		// 可见侧失去 F14/F27 的 ZW 直写形态（安全优先，口径按 P3c 先例
		// 校准）；语义不变。
		let mut zw_inner = String::new();
		for (fi, sites) in nop_sites.iter().enumerate() {
			for &p in sites {
				zw_inner.push_str(&format!(
					"PF_[{}].W[{}] = NOPA; ",
					fi + 1,
					p as usize + 1,
				));
			}
		}
		// ㉒ 编码体（㉓ 起内联于 BSS 碎片）：五流加性掩码 + 常量编码 +
		// Closure 扫描链原型树，返回编码态树根。
		// B-3 (状态链式按块解码 — 阶段一: 按块加密集散 + 会话钥匙):
		// 每原型的指令流按基本块分割（编译器给的 leader 表），每块用
		// (PS + i*PT + block_start*PB + SALT) 种子独立加密。SALT 是 boot
		// 期随机折叠（新表地址）——**静态文件里不存在任何能推出块钥匙的
		// 材料**，批量解码器随之消亡；跨运行钥匙全变，收集品一次性。
		let bl_lists: Vec<String> = block_starts
			.iter()
			.map(|starts| {
				format!(
					"{{{}}}",
					starts
						.iter()
						.map(|v| v.to_string())
						.collect::<Vec<_>>()
						.join(", ")
				)
			})
			.collect();
		let enc_body = format!(
			"local P = PF_ local EP = {{}} local SALT = 0 do local _st = {{}} local _ts = TOSTR(_st) for _i = 1, #_ts do SALT = (SALT * 31 + BYTE(_ts, _i)) % 268435456 end end local BL = {{ {blists} }} for i = 1, #P do local pf = P[i] local bs = BL[i] local nb = #bs local bl, bo, bm = {{}}, {{}}, {{}} for bi = 1, nb do local s0 = bs[bi] local e0 = (bi < nb) and (bs[bi + 1] - 1) or #pf.W bl[bi] = e0 - s0 + 1 bm[s0] = bi bo[bi] = (bi > 1) and (bo[bi - 1] + bl[bi - 1] * 5) or 1 end local Wt, ks = pf.W, pf.{bs_stream} local km = {{}} for j = 1, #Wt do if Wt[j] == {cw} then km[#km + 1] = ks[j] + 1 end end local bx = {{}} local xo = 1 for bi = 1, nb do local s0 = bs[bi] local st = (PS + i * PT + s0 * PB + SALT) % 268435456 for p = s0, s0 + bl[bi] - 1 do st = (KM * st + KC) % 268435456 bx[xo] = (pf.W[p] + st % 65536) % 65536 xo = xo + 1 st = (KM * st + KC) % 268435456 bx[xo] = (pf.SA[p] + st % 65536) % 65536 xo = xo + 1 st = (KM * st + KC) % 268435456 bx[xo] = (pf.SB[p] + st % 65536) % 65536 xo = xo + 1 st = (KM * st + KC) % 268435456 bx[xo] = (pf.SC[p] + st % 65536) % 65536 xo = xo + 1 st = (KM * st + KC) % 268435456 bx[xo] = (pf.SD[p] + st % 65536) % 65536 xo = xo + 1 end end local st = (PS + i * PT + SALT) % 268435456 {cblock} EP[i] = {{ bs = bs, bl = bl, bo = bo, bm = bm, bx = bx, sd = i, S = pf.S, upsrc = pf.upsrc, nparams = pf.nparams, k = {{}}, km = km, {cfields} }} end for i = 1, #EP do local km = EP[i].km for j = 1, #km do EP[i].k[km[j]] = EP[km[j]] end EP[i].km = nil end return EP[#EP], SALT",
			blists = bl_lists.join(", "),
			bs_stream = s_of[1],
			cw = closure_wire,
			cblock = c_block_enc,
			cfields = c_fields_enc,
		);
		// ㉓: BSS 碎片 = 解析 + 校验 + 自改写 + 重编码，一步到位——
		// 解码态原型表只存在于这个 loadstring 出的加密碎片内部。
		// ㉚ B-2 (惩罚形态升级，攻击方约束之 4): 字节码校验和失配不再
		// `while true do end`（grep 即得的死循环 oracle）——改为毒化块
		// 步长 PB：加密侧（本碎片）与解密侧（RT 碎片的 RPK5）从此错位，
		// 程序照跑、每个块的指令全错，且无任何可见比较/循环可补。
		let ck_poison = rng.int(1, 268_435_455);
		manifest_key("BSS_CKPOISON", ck_poison as u64);
		let bs_src = format!(
			"return function(BYTE, CHAR, FLR, SUB, AL, TK, decarrier, r16, CKM, CKC, BKM, BKC, BSEED, BSTEP, TYP, TOSTR) local OS = {{{}}} {} return function(FN_, NOPA, PS, PT, KM, KC, PB) local PF_ = {{}} local di = 1 while di <= #FN_ do PF_[di] = parse(FN_[di], di); if PF_[di].ck ~= OS[di] then PB = (PB + {ckpo}) % 268435456 end; di = di + 1 end {} {} end end",
			os_list, parse_fn, zw_inner, enc_body, ckpo = ck_poison
		);
		frags.push((200u16, bs_src.into_bytes()));
		// DDEC（wire 206）：每帧解码，工厂形态把钥匙以 boot 期上游值
		// 注入（钥匙表事后置 nil 不影响）。与编码钥匙流严格镜像。
		let c_block_dec = if consts_safe {
			String::from(
				"local C = {} local ct, cn, csb, csl = q.ct, q.cn, q.csb, q.csl local si = 1 for j = 1, q.m do st = (KM * st + KC) % 268435456 local t = (ct[j] - st) % 65536 if t == 1 then C[j] = cn[j] - st elseif t == 2 or t == 5 then local l = csl[j] local b = {} for x = 1, l do st = (KM * st + KC) % 268435456 b[x] = (csb[si] - st % 256) % 256 si = si + 1 end if t == 2 then C[j] = CHAR(UNP(b, 1, l)) else C[j] = TONUM(CHAR(UNP(b, 1, l))) end elseif t == 3 then C[j] = true elseif t == 4 then C[j] = false end end",
			)
		} else {
			String::from("local C = q.C")
		};
		// ㉗ (R009/Suda 回合 — 运行时入密): DDEC 不再是独立可见碎片
		// （其可见调用点曾是文本手术探针位）——解码器函数体内联进运行
		// 时碎片（wire 208），PS/PT/KM/KC 作为碎片工厂参数（可见层只见
		// 钥匙装配数值，不见解码器函数值）。
		// B-3: 整流批量解码器废除。CDEC 只解常量池（种子含会话盐，
		// 与任何块钥匙不同构：无 PB 项）；指令按块在 bdec 里即需即解。
		// ㉚ B-2: 常量池走 SLTC（链终值第二约束），与指令块的 SLT 分离。
		let cdec_inner = format!(
			"function(q, CHAR, UNP, TONUM) local st = (PS + q.sd * PT + SLTC) % 268435456 {cblock} return C end",
			cblock = c_block_dec,
		);
		let bdec_src = String::from(
			"local function bdec(E, bi) local c = E.bch[bi] if c then E.ww, E.w0, E.w1, E.w2, E.w3 = c[1], c[2], c[3], c[4], c[5] E.bp, E.be = c[6], c[7] return end local pf = E.bt local s0 = pf.bs[bi] local l = pf.bl[bi] local st = (PS + pf.sd * PT + s0 * PB + SLT) % 268435456 local t0, t1, t2, t3, t4 = {}, {}, {}, {}, {} local x = pf.bx local off = pf.bo[bi] for i = 1, l do st = (KM * st + KC) % 268435456 t0[i] = (x[off] - st % 65536) % 65536 off = off + 1 st = (KM * st + KC) % 268435456 t1[i] = (x[off] - st % 65536) % 65536 off = off + 1 st = (KM * st + KC) % 268435456 t2[i] = (x[off] - st % 65536) % 65536 off = off + 1 st = (KM * st + KC) % 268435456 t3[i] = (x[off] - st % 65536) % 65536 off = off + 1 st = (KM * st + KC) % 268435456 t4[i] = (x[off] - st % 65536) % 65536 off = off + 1 end E.bch[bi] = { t0, t1, t2, t3, t4, s0, s0 + l - 1 } E.ww, E.w0, E.w1, E.w2, E.w3 = t0, t1, t2, t3, t4 E.bp, E.be = s0, s0 + l - 1 end",
		);
		// ㉗ (R009/Suda 回合 — 运行时入密): newE/makefn/run+蹦床+DDEC
		// 整体打包为加密运行时碎片（wire 208）。工厂参数 = 钥匙数值×4
		// + 不透明依赖值（原语表/环境函数/续行与执行器×8 个离散函数
		// 值）。可见层从此没有任何一行代码触碰解码数据：文本手术探针
		// （R008/Suda 实测的三处插入点）全部失去目标。
		// B-3: newE 不再整流解码——常量即解，指令只解入口块；块缓存
		// 挂帧（E.bch），帧退即毁。
		let rt_newe = format!(
			"newE = function(pf, V, ups, vargs, vargc)\n    {}\n    local C = CDEC(pf, CHAR, UNP, TONUM)\n    local S = pf.S\n    local O = {{}}\n    local E = {}\n    E.bt = pf\n    E.bm = pf.bm\n    E.bch = {{}}\n    E.bdec = bdec\n    bdec(E, 1)\n    return E\n  end\n  ",
			run_unpack, e_table
		);
		// ㉚ B-2: 链终值 (H1, H2) + 归零常量 (CB*/CC*) 以工厂参数入密。
		// 前两行 = 哈希链落点：源码完整 ⇒ 两个偏移都是 0，钥匙流原样；
		// 任何源码被改 ⇒ D_b/D_c 非零 ⇒ bdec（指令块）与 CDEC（常量池）
		// 各持一条独立约束——单约束 28bit，双约束合计 ~52bit 二预映像。
		// SLTC 与 SLT 分离：常量池钥匙流单独偏移，攻击者无法用一个
		// 全局常数补偿两条流。
		let rt_src = format!(
			"return function(PS, PT, KM, KC, PB, SLT, P, G, U, FLOOR, MS, AV, HW2, TP, mget, resolve_call, callcap, HAS_LEN_META, XC1, XC2, XC3, XC4, XE1, XE2, XE3, XE4, CLK, H1, H2, CB1, CB2, CBC, CC1, CC2, CCC)\n  SLT = (SLT + (H1 * CB1 + H2 * CB2 + CBC) % 268435456) % 268435456\n  local SLTC = (SLT + (H1 * CC1 + H2 * CC2 + CCC) % 268435456) % 268435456\n  local CDEC = {dec}\n  {bdec}\n  local newE\n  {mk}  {ne}local run = function(pf, V, ups, vargs, vargc)\n    local E = newE(pf, V, ups, vargs, vargc)\n    {loop}\n  end\n  return run\nend",
			dec = cdec_inner,
			bdec = bdec_src,
			mk = makefn_decl,
			ne = rt_newe,
			loop = trampoline_src.clone().unwrap(),
		);
		frags.push((208u16, rt_src.into_bytes()));
		// ㉘-A: 碎片源码净化（注释剥离 + 全标识符改名 + 单行化）。
		// 保留集 = Lua 关键字 + 运行时固定全局名；改名映射跨全部碎片
		// 一致（字段名跨碎片共享，必须同名）。
		let mut frag_reserved: std::collections::HashSet<String> =
			crate::mangle::RESERVED.iter().map(|x| x.to_string()).collect();
		for g in [
			"pairs", "ipairs", "next", "select", "unpack", "type", "tostring",
			"tonumber", "print", "error", "assert", "pcall", "xpcall",
			"setmetatable", "getmetatable", "rawget", "rawset", "rawequal",
			"string", "table", "math", "bit32", "os", "debug", "buffer",
			"task", "coroutine", "loadstring", "load", "getfenv", "setfenv",
			"newproxy", "collectgarbage", "warn", "utf8", "typeof", "_G",
			"_VERSION", "Vector3", "Vector2", "Instance", "Enum", "game",
			"workspace", "tick", "wait",
		] {
			frag_reserved.insert(g.to_string());
		}
		let mut frag_map: std::collections::HashMap<String, String> =
			std::collections::HashMap::new();
		let mut frag_used: std::collections::HashSet<String> =
			std::collections::HashSet::new();
		for (_wire, src) in frags.iter_mut() {
			let text = String::from_utf8(std::mem::take(src)).unwrap();
			*src = sanitize_frag(&text, rng, &mut frag_map, &frag_reserved, &mut frag_used)
				.into_bytes();
		}
		// ㉚ B-2: 每碎片自由填充尾注（14–22B 随机）——回填搜索空间
		// + 每构建长度熵；尾注随源码一起进哈希链。
		for (_wire, src) in frags.iter_mut() {
			frag_pad(src, rng);
		}
		// ㉘ 研究/测试钩子: 转储净化后的碎片源码（环境门控，默认关）。
		// ㉚: 每片一行（尾注填充在行尾，检查侧可按 $ 锚定剥离）。
		if let Ok(dir) = std::env::var("LURAPH_FRAG_SAN") {
			for (wire, src) in frags.iter() {
				let mut out = src.clone();
				out.push(b'\n');
				std::fs::write(format!("{}/san_{:03}.src", dir, wire), &out).unwrap();
			}
		}
		// mask + base-94 pack. B-2 (㉚): per-fragment keystream seed =
		// (hseed + wire*hstep + rv_prev*sh_mul) % 2^28，rv_prev = 此前
		// 全部已解出源码的链式哈希（HBOOT 自身源码为链头）。篡改任何
		// 一片 ⇒ 其后全部碎片钥匙错位；链上无任何比较语句——哈希就是
		// 钥匙本身（攻击方四约束之 1/3）。
		let hstep = rng.int(1_048_576, 268_435_455) as u32;
		let alpha = carrier.alphabet;
		let mut slots: Vec<i64> = (1..=2000).collect();
		rng.shuffle(&mut slots);
		// 增量⑩: handler-fragment keystream keys KF-assembled, anchored
		// on #hqi (the fragment-index table declared in the same block).
		// NOTE: hqi is a Vec of "w, s, l" TRIPLET strings that join into
		// a flat table — runtime #hqi = 3 * hqi.len().
		// （㉚ 前移：HBOOT 模板必须在碎片加密之前定型——它是链头）
		let n_hqi = 3 * frags.len() as i64;
		let hseed_e = ke.key_expr(hseed as i64, Some(("#hqi", n_hqi)), rng);
		let hstep_e = ke.key_expr(hstep as i64, Some(("#hqi", n_hqi)), rng);
		let hm_e = ke.key_expr(hm as i64, Some(("#hqi", n_hqi)), rng);
		let hc_e = ke.key_expr(hc as i64, Some(("#hqi", n_hqi)), rng);
		manifest_key("HQ_SEED", hseed as u64);
		manifest_key("HQ_STEP", hstep as u64);
		manifest_key("HQ_KM", hm as u64);
		manifest_key("HQ_KC", hc as u64);
		// ㉚ B-2: 哈希链常量——每构建随机乘数/初值/链系数，全部经
		// #hqi 锚点装配（HBOOT 内零字面量）。k/mul < 2^24 → Lua 侧
		// 所有乘积 < 2^52 精确（double 无损）。
		let sh_k1 = (rng.int(1_048_576, 16_777_215) | 1) as u64;
		let sh_k2 = (rng.int(1_048_576, 16_777_215) | 1) as u64;
		let sh_iv1 = rng.int(0, SH_M1 as i64 - 1) as u64;
		let sh_iv2 = rng.int(0, SH_M2 as i64 - 1) as u64;
		let sh_mul = (rng.int(1_048_576, 16_777_215) | 1) as u64;
		let sh_k1_e = ke.key_expr(sh_k1 as i64, Some(("#hqi", n_hqi)), rng);
		let sh_k2_e = ke.key_expr(sh_k2 as i64, Some(("#hqi", n_hqi)), rng);
		let sh_iv1_e = ke.key_expr(sh_iv1 as i64, Some(("#hqi", n_hqi)), rng);
		let sh_iv2_e = ke.key_expr(sh_iv2 as i64, Some(("#hqi", n_hqi)), rng);
		let sh_mul_e = ke.key_expr(sh_mul as i64, Some(("#hqi", n_hqi)), rng);
		manifest_key("CHAIN_K1", sh_k1);
		manifest_key("CHAIN_K2", sh_k2);
		manifest_key("CHAIN_MUL", sh_mul);
		manifest_key("CHAIN_IV1", sh_iv1);
		manifest_key("CHAIN_IV2", sh_iv2);
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
		// ㉚ B-2 (源码自校验哈希链): HBOOT 新增职责——
		//   * 收 HB 参数（自身源码，可见引导经 table.concat(MH) 传入）
		//     并先对它走链 → 换掉 HBOOT 即链头偏移；
		//   * 每片解码钥匙揉入「此前全部源码」的链值（rv * sh_mul）
		//     → 篡改前片 ⇒ 后片全部解出垃圾；
		//   * 每片解码后、loadstring 前对解码态源码走链；
		//   * 返回链终值 (h1, h2) → 可见引导转交 RT 碎片，在块钥匙流
		//     里做**双独立归零约束**（指令块一条、常量池一条，合计
		//     ~52bit 二预映像难度）——期望哈希不落任何明文常量。
		//   SH 助手 = 双车道非线性步（Rust 镜像 sh_step，逐位一致）。
		let hboot_src = format!(
			"return function(HQ, hqi, AL, BYTE, CHAR, FLR, SUB, LS, KA, KB, KC, KM, HB) local HW = {{}} local BSS local h1 = {iv1} local h2 = {iv2} local SH = function(a, c, b) a = (a * {k1} + b * (c % 97 + 1)) % {m1} c = (c * {k2} + b * (a % 89 + 1)) % {m2} return a, c end do local hn = #HB for i = 1, hn do h1, h2 = SH(h1, h2, BYTE(HB, i)) end end local DICT = HB local hi = 1 while hi <= #hqi do local w = hqi[hi] local seg = HQ[hqi[hi + 1]] local flen = hqi[hi + 2] hi = hi + 3 local hs = ({hseed} + w * {hstep} + ((h1 + h2 * 257) % 268435456) * {hmul}) % 268435456 local t = {{}} local ti = 1 local n = #seg for i = 1, n, 5 do local v = 0 v = v * 94 + AL[BYTE(seg, i)] v = v * 94 + AL[BYTE(seg, i + 1)] v = v * 94 + AL[BYTE(seg, i + 2)] v = v * 94 + AL[BYTE(seg, i + 3)] v = v * 94 + AL[BYTE(seg, i + 4)] local b1 = v % 256; v = FLR(v / 256) local b2 = v % 256; v = FLR(v / 256) local b3 = v % 256; v = FLR(v / 256) local b4 = v % 256 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b1 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b2 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b3 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 hs = ({hm} * hs + {hc}) % 268435456; t[ti] = CHAR((b4 - (((hs % 256) + (FLR(hs / 256) % 256) + (FLR(hs / 65536) % 256) + FLR(hs / 16777216)) % 256)) % 256); ti = ti + 1 end local s = SUB(table.concat(t), 1, flen) local DC = {{}} local pi = 3 while pi <= flen do if BYTE(s, pi) == 0 then local ln = BYTE(s, pi + 1) + BYTE(s, pi + 2) * 256 DC[#DC + 1] = SUB(s, pi + 3, pi + 2 + ln) pi = pi + 3 + ln else local off = BYTE(s, pi + 1) + BYTE(s, pi + 2) * 256 local ln = BYTE(s, pi + 3) + BYTE(s, pi + 4) * 256 DC[#DC + 1] = SUB(DICT, off, off + ln - 1) pi = pi + 5 end end s = SUB(table.concat(DC), 1, BYTE(s, 1) + BYTE(s, 2) * 256) DICT = DICT .. s for i = 1, #s do h1, h2 = SH(h1, h2, BYTE(s, i)) end if w == 200 then BSS = s else HW[w] = LS(s)() end end return HW, BSS, h1, h2 end",
			hseed = hseed_e, hstep = hstep_e, hm = hm_e, hc = hc_e,
			k1 = sh_k1_e, k2 = sh_k2_e, m1 = SH_M1, m2 = SH_M2,
			iv1 = sh_iv1_e, iv2 = sh_iv2_e, hmul = sh_mul_e,
		);
		// ㉘-A: HBOOT 元碎片同样净化（名字/注释不泄漏）；㉚: 自由填充
		// 尾注，随后其字节作为链头进哈希。
		let mut hb_bytes =
			sanitize_frag(&hboot_src, rng, &mut frag_map, &frag_reserved, &mut frag_used)
				.into_bytes();
		frag_pad(&mut hb_bytes, rng);
		// ㉚: 链初值 = HBOOT 自身源码的哈希（boot 以 HB 参数回传同一串）
		let (mut ch1, mut ch2) = sh_bytes(&hb_bytes, sh_iv1, sh_iv2, sh_k1, sh_k2);
		// ㉚: 解码顺序 = hqi 洗牌序；Rust 侧按同一顺序走链并逐片加密。
		let mut order: Vec<usize> = (0..frags.len()).collect();
		rng.shuffle(&mut order);
		let mut hq_lines = String::from("local HQ = {}\n");
		let mut hqi_vals: Vec<i64> = Vec::new();
		// ㉟: 增量链字典 = HBOOT 自身源码开头（解码侧以 HB 同样初始化），
		// 其后每片解码态源码依次并入——与 HBOOT 的重建严格同序。
		let mut lz_dict: Vec<u8> = hb_bytes.clone();
		for (_pos, &fi) in order.iter().enumerate() {
			let (_wire, src) = &frags[fi];
			let rv = (ch1 + ch2 * 257) % 268_435_456;
			let mut state = ((hseed as u64 + *_wire as u64 * hstep as u64 + rv * sh_mul)
				% 268_435_456) as u64;
			// ㉟: 加密对象 = 增量链压缩流（HBOOT 先重建源码再走原链）；
			// ㉚ 链哈希照旧走在**重建后**的解码态源码上（见下方链推进）。
			let payload = lz_chain_encode(src, &lz_dict);
			#[cfg(debug_assertions)]
			{
				let back = lz_chain_decode_check(&payload, &lz_dict);
				assert!(
					back == *src,
					"lz roundtrip fail: wire {} len {} vs {}",
					_wire,
					back.len(),
					src.len()
				);
			}
			lz_dict.extend_from_slice(src);
			let mut xb: Vec<u8> = payload
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
			let slot = slots[fi];
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
			hqi_vals.push(*_wire as i64);
			hqi_vals.push(slot);
			hqi_vals.push(blen as i64);
			// ㉚: 链推进——本片解码态源码（含填充尾注）入链；下一片
			// 的钥匙种子里的 rv 即「此前全部源码」的链值。
			let (a, c) = sh_bytes(src, ch1, ch2, sh_k1, sh_k2);
			ch1 = a;
			ch2 = c;
		}
		// ㉚ B-2: 链终值 (sh1, sh2) = 全部源码完整性的期望锚点。构建期
		// 由此派生两组归零常量（ CBC/CCC = -(H·C) mod 2^28 ）；运行期
		// HBOOT 走同一条链把终值交回，RT 碎片内做
		//   D_b = (H1*CB1 + H2*CB2 + CBC) % 2^28 → 揉进指令块钥匙流
		//   D_c = (H1*CC1 + H2*CC2 + CCC) % 2^28 → 揉进常量池钥匙流
		// 源码未动 ⇒ D_b = D_c = 0（程序原样跑）；任何一层源码被改 ⇒
		// 双约束同时非零 ⇒ 解密结构完好、内容全错（攻击方约束之 4）。
		// 期望哈希不以明文常量落盘——回填重算无目标可打（约束之 1）。
		// 系数 < 2^25 → H*C 乘积 < 2^51，Lua 侧精确。
		if std::env::var("LURAPH_LZ_DBG").is_ok() {
			eprintln!("LZEXP {},{}", ch1, ch2);
		}
		let sh1 = ch1 as i64;
		let sh2 = ch2 as i64;
		let sh_cb1 = rng.int(1_048_576, 33_554_431) | 1;
		let sh_cb2 = rng.int(1_048_576, 33_554_431) | 1;
		let sh_cc1 = rng.int(1_048_576, 33_554_431) | 1;
		let sh_cc2 = rng.int(1_048_576, 33_554_431) | 1;
		let sh_cbc = (KEY_MOD - (sh1 * sh_cb1 + sh2 * sh_cb2) % KEY_MOD) % KEY_MOD;
		let sh_ccc = (KEY_MOD - (sh1 * sh_cc1 + sh2 * sh_cc2) % KEY_MOD) % KEY_MOD;
		let sh_cb1_e = ke.key_expr(sh_cb1, None, rng);
		let sh_cb2_e = ke.key_expr(sh_cb2, None, rng);
		let sh_cbc_e = ke.key_expr(sh_cbc, None, rng);
		let sh_cc1_e = ke.key_expr(sh_cc1, None, rng);
		let sh_cc2_e = ke.key_expr(sh_cc2, None, rng);
		let sh_ccc_e = ke.key_expr(sh_ccc, None, rng);
		manifest_key("SH_CB1", sh_cb1 as u64);
		manifest_key("SH_CB2", sh_cb2 as u64);
		manifest_key("SH_CBC", sh_cbc as u64);
		manifest_key("SH_CC1", sh_cc1 as u64);
		manifest_key("SH_CC2", sh_cc2 as u64);
		manifest_key("SH_CCC", sh_ccc as u64);
		// ㉛ (R014 回合 — P2-6): hqi 段表（wire/槽位/长度三元组——攻击方
		// 原话「最贵的一份结构信息，白送」）改为加性 LCG 掩码落盘，引导
		// 期即解。静态侦察拿不到段数/长度/槽位；攻击方固化的段表正则失效。
		let hqi_km = rng.int(1_048_577, 33_000_001) | 1;
		let hqi_ks0 = rng.int(1_048_576, KEY_MOD - 1);
		let hqi_kc = rng.int(1_048_576, KEY_MOD - 1);
		let hqi_km_e = ke.key_expr(hqi_km, None, rng);
		let hqi_ks0_e = ke.key_expr(hqi_ks0, None, rng);
		let hqi_kc_e = ke.key_expr(hqi_kc, None, rng);
		manifest_key("HQI_KM", hqi_km as u64);
		manifest_key("HQI_KS", hqi_ks0 as u64);
		manifest_key("HQI_KC", hqi_kc as u64);
		let mut hks = hqi_ks0;
		let hqi_masked: Vec<String> = hqi_vals
			.iter()
			.map(|v| {
				hks = (hqi_km * hks + hqi_kc) % KEY_MOD;
				(v + hks).to_string()
			})
			.collect();
		let hqi_unmask = format!(
			"do\n      local hqs, hqm, hqc = {}, {}, {}\n      for hqi_ = 1, #hqi do hqs = (hqm * hqs + hqc) % 268435456 hqi[hqi_] = hqi[hqi_] - hqs end\n    end\n    ",
			hqi_ks0_e, hqi_km_e, hqi_kc_e
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
		let v_os = "hos";
		let v_clock = "hclk";
		let v_s = "hsarg";
			let v_ts = "hts";
		boot.push_str(&coded_name_tpl(rng, v_ts, "tostring"));
		boot.push_str(&coded_name_tpl(rng, v_ls, "loadstring"));
		boot.push_str(&coded_name_tpl(rng, v_dbg, "debug"));
		boot.push_str(&coded_name_tpl(rng, v_inf, "info"));
		boot.push_str(&coded_name_tpl(rng, v_s, "s"));
		boot.push_str(&coded_name_tpl(rng, v_os, "os"));
		boot.push_str(&coded_name_tpl(rng, v_clock, "clock"));
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
		// ㉔-1 (R008 回合 — 守卫结果钥匙化): R008 实测攻击第 2 步 = 把
		// 可见布尔标志补成通过（"重置 tY = true"）。废除标志与死循环
		// oracle：loadstring 原生性探测的结果折进 pb 钥匙流——干净环境
		// pb == c_fold（探针项恒零）；pcall 失败/被钩换源 → pb 偏移 →
		// 元钥匙流污染 → HBOOT 解出垃圾 → 错钥静默死亡。补标志不再
		// 有目标可打（标志已不存在）。
		let nlok_delta = rng.int(1_048_576, KEY_MOD - 1);
		let nlok_delta_e = ke.key_expr(nlok_delta, None, rng);
		manifest_key("NLOK_DELTA", nlok_delta as u64);
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
		// ㉔-2 (R008 回合 — 环境值绑定): R008 实测攻击第 1 步 = 给
		// `task.defer`/`Vector3.new`/`Vector2.new` 塞**哑桩**（任意返回
		// 值），模块表构造过了就完事。㉑ 只绑「存在性」；本轮把目标运行时
		// 的**值**揉进元钥匙流：boot 用每构建随机参数调 `Vector3.new`/
		// `Vector2.new`，对返回对象的 `.X/.Y/.Z` 分量做每构建随机系数
		// 线性折叠——哑桩（不读构造参数）折出垃圾值 → 元钥匙流污染 →
		// HBOOT 解出垃圾 → 错钥静默死亡。桩必须**忠实实现构造语义**
		// （正确回传分量）才能过关。分量算术全整数精确（float32 分量
		// 存小整数无损，Lua 侧 double 运算精确），目标运行时/沙箱一致。
		// 诚实边界：受控沙箱若完整仿真这些 API 仍能过（R004 定律）——
		// 抬的是「任意桩」到「忠实仿真」的成本。
		// ㉛ (R014 回合 — P0-1): 「(x - K) * mix」减法零化形被攻击方一眼
		// 识破（正常态恒零 → 整项可忽略，反钩防线形同虚设）。改为分裂
		// 乘积直参形：x = x_lo + x_hi·2^16，两项各自乘预混常量——哈希
		// **直接**参与种子（攻击方建议 B 的形态），零化量折进总补偿常数
		// （SEED_COMP，KT 装配）。数值边界：单积 < 2^16·2^28 = 2^44，
		// 七项和 < 2^47，double 全程精确。
		let (env_gate, env_term, env_comp) = if bind_env.is_some() {
			let ea: Vec<i64> = (0..5).map(|_| rng.int(1, 999)).collect();
			let ec: Vec<i64> = (0..6).map(|_| rng.int(1, 999)).collect();
			let esum = (ea[0] * ec[0]
				+ ea[1] * ec[1]
				+ ea[2] * ec[2]
				+ ea[3] * ec[3]
				+ ea[4] * ec[4])
				% KEY_MOD;
			let env_exp = (esum * ec[5]) % KEY_MOD;
			let env_mix = rng.int(1_048_576, KEY_MOD - 1);
			let env_mix_e = ke.key_expr(env_mix, None, rng);
			let env_mix_hi = (env_mix * 65536) % KEY_MOD;
			let env_mix_hi_e = ke.key_expr(env_mix_hi, None, rng);
			manifest_key("ENV_EXP", env_exp as u64);
			manifest_key("ENV_MIX", env_mix as u64);
			manifest_key("ENV_MIX_HI", env_mix_hi as u64);
			let gate = format!(
				"local ef1 = 0\n    do\n      local v3 = Vector3.new({}, {}, {})\n      local v2 = Vector2.new({}, {})\n      ef1 = ((v3.X * {} + v3.Y * {} + v3.Z * {} + v2.X * {} + v2.Y * {}) * {}) % 268435456\n    end\n    ",
				obf_num(ea[0] as u64, rng),
				obf_num(ea[1] as u64, rng),
				obf_num(ea[2] as u64, rng),
				obf_num(ea[3] as u64, rng),
				obf_num(ea[4] as u64, rng),
				obf_num(ec[0] as u64, rng),
				obf_num(ec[1] as u64, rng),
				obf_num(ec[2] as u64, rng),
				obf_num(ec[3] as u64, rng),
				obf_num(ec[4] as u64, rng),
				obf_num(ec[5] as u64, rng),
			);
			(
				gate,
				format!(
					" + (ef1 % 65536) * {} + FLR(ef1 / 65536) * {}",
					env_mix_e, env_mix_hi_e
				),
				(env_exp * env_mix) % KEY_MOD,
			)
		} else {
			(String::new(), String::new(), 0)
		};
		// ㉛: pb 项分裂乘积的高半常量（pmx·2^16 mod M）
		let probe_mix_hi = (probe_mix * 65536) % KEY_MOD;
		let probe_mix_hi_e = ke.key_expr(probe_mix_hi, None, rng);
		manifest_key("PROBE_MIX_HI", probe_mix_hi as u64);
		let (ak_gate, meta_seed_full) = if bind_key.is_some() {
			let bind_mix = rng.int(1_048_576, KEY_MOD - 1);
			let bind_mix_e = ke.key_expr(bind_mix, Some(("#hqi", n_hqi)), rng);
			let bind_mix2 = rng.int(1_048_576, KEY_MOD - 1);
			let bind_mix2_e = ke.key_expr(bind_mix2, Some(("#hqi", n_hqi)), rng);
			let bind_mix_hi = (bind_mix * 65536) % KEY_MOD;
			let bind_mix_hi_e = ke.key_expr(bind_mix_hi, Some(("#hqi", n_hqi)), rng);
			let bind_mix2_hi = (bind_mix2 * 65536) % KEY_MOD;
			let bind_mix2_hi_e = ke.key_expr(bind_mix2_hi, Some(("#hqi", n_hqi)), rng);
			manifest_key("BIND_AK", ak_fold as u64);
			manifest_key("BIND_AK2", ak2_fold as u64);
			manifest_key("BIND_MIX", bind_mix as u64);
			manifest_key("BIND_MIX2", bind_mix2 as u64);
			manifest_key("BIND_MIX_HI", bind_mix_hi as u64);
			manifest_key("BIND_MIX2_HI", bind_mix2_hi as u64);
			// ㉛: 总补偿常数 = meta_seed − Σ(目标值·混元)（mod 2^28）。
			// 干净环境各探针折出目标值 ⇒ 种子恰还原 meta_seed；任何
			// 探针被钩/环境失真 ⇒ 对应直参项偏移 ⇒ 元钥匙流污染。
			let comp = (meta_seed - c_fold * probe_mix - env_comp
				- (ak_fold * bind_mix) % KEY_MOD
				- (ak2_fold * bind_mix2) % KEY_MOD)
				.rem_euclid(KEY_MOD);
			let comp_e = ke.key_expr(comp, None, rng);
			manifest_key("SEED_COMP", comp as u64);
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
				"({} + (pb % 65536) * {} + FLR(pb / 65536) * {}{} + (ak1 % 65536) * {} + FLR(ak1 / 65536) * {} + (ak2 % 65536) * {} + FLR(ak2 / 65536) * {}) % 268435456",
				comp_e, probe_mix_e, probe_mix_hi_e, env_term,
				bind_mix_e, bind_mix_hi_e, bind_mix2_e, bind_mix2_hi_e
			);
			(gate, seed)
		} else {
			// ㉛: 无绑定形态——补偿 = meta_seed − c_fold·pmx − env 补偿
			let comp = (meta_seed - c_fold * probe_mix - env_comp).rem_euclid(KEY_MOD);
			let comp_e = ke.key_expr(comp, None, rng);
			manifest_key("SEED_COMP", comp as u64);
			(
				String::new(),
				format!(
					"({} + (pb % 65536) * {} + FLR(pb / 65536) * {}{}) % 268435456",
					comp_e, probe_mix_e, probe_mix_hi_e, env_term
				),
			)
		};
		// ㉛: MP 程序字掩码钥匙（每构建随机，KT 装配）
		let mpm_seed = rng.int(1_048_576, KEY_MOD - 1);
		let mpm_m = rng.int(1_048_577, 33_000_001) | 1;
		let mpm_c = rng.int(1_048_576, KEY_MOD - 1);
		let mpm_seed_e = ke.key_expr(mpm_seed, None, rng);
		let mpm_m_e = ke.key_expr(mpm_m, None, rng);
		let mpm_c_e = ke.key_expr(mpm_c, None, rng);
		manifest_key("MPM_SEED", mpm_seed as u64);
		manifest_key("MPM_M", mpm_m as u64);
		manifest_key("MPM_C", mpm_c as u64);
		let metavm = emit_metavm(
			&meta_seed_full, &meta_m_e, &meta_c_e, hb_masked.len(), rng,
			mpm_seed, mpm_m, mpm_c, &mpm_seed_e, &mpm_m_e, &mpm_c_e,
		);
		// 增量⑲: build-time-shuffled continuation wiring (which CT slot
		// holds which encrypted re-entry fragment is per-build random).
		let mut cont_wires: Vec<u16> = (301..305).collect();
		rng.shuffle(&mut cont_wires);
		// ㉗: 续行碎片装进离散可见局部（不再经表索引——表是包装手术点）
		let ct_fill: String = (0..4)
			.map(|i| format!("CT{} = HW[{}]\n    ", i + 1, cont_wires[i]))
			.collect();
		// call executors: fixed callx->wire mapping (the chain phases
		// stash E.callx = 1..4), emit order shuffled (cosmetic)
		let mut cx_order: Vec<usize> = (0..4).collect();
		rng.shuffle(&mut cx_order);
		let cx_fill: String = cx_order
			.iter()
			.map(|&i| format!("CX{} = HW[{}]\n    ", i + 1, 305 + i))
			.collect();
		let build = format!(
			r#"{}  {}local HW = {{}}
  local BSS
  local AV = 0
  local HW2 = {{}}
  local CT1, CT2, CT3, CT4
  local CX1, CX2, CX3, CX4
  local RTFRAG
  local RPK1, RPK2, RPK3, RPK4, RPK5, RPK6, RPK7, RPK8, RPK9, RPK10, RPK11
  local SALT
  local SH1, SH2
  local MPF
  do
    {}local LS = GFE(0)[{v_ls}]
    local TSTR = GFE(0)[{v_ts}]
    local DBG = GFE(0)[{v_dbg}]
    local INF = DBG and DBG[{v_inf}]
    local OS_ = GFE(0)[{v_os}]
    local CLK = OS_ and OS_[{v_clock}]
    local pb = 0
    do
      local ok, sr = PCAL(function() return INF(LS, {v_s}) end)
      if ok and TYP(sr) == {strlit} then
        for i = 1, #sr do pb = (pb * 31 + BYTE(sr, i)) % 268435456 end
      end
      pb = (pb + (ok and 0 or {nlok_delta})) % 268435456
    end
    {env_gate}{ak_gate}local hqi = {{{}}}
    {hqi_unmask}{}    {}
    local HB = table.concat(MH)
    HW, BSS, SH1, SH2 = LS(HB)()(HQ, hqi, AL, BYTE, CHAR, FLR, SUB, LS, KA, KB, KC, KM, HB)
    HQ = nil; hqi = nil
    {ct_fill}{cx_fill}RTFRAG = HW[208]
    RPK1 = {pseed}
    RPK2 = {pstep}
    RPK3 = {pkm}
    RPK4 = {pkc}
    RPK5 = {pblock}
    RPK6 = {shcb1}
    RPK7 = {shcb2}
    RPK8 = {shcbc}
    RPK9 = {shcc1}
    RPK10 = {shcc2}
    RPK11 = {shccc}
    do
      local avt = {{}}
      local ats = TSTR(avt)
      for i = 1, #ats do AV = (AV * 31 + BYTE(ats, i)) % 268435456 end
    end
    for w, f in pairs(HW) do if w < 256 then HW2[(w + AV) % 256] = f end end
    HW = nil
    MPF, SALT = LS(BSS)()(BYTE, CHAR, FLR, SUB, AL, TK, decarrier, r16, CKM, CKC, BKM, BKC, BSEED, BSTEP, TYP, TSTR)(FN, NOPA, {pseed}, {pstep}, {pkm}, {pkc}, {pblock})
    BSS = nil
  end"#,
			hq_lines, mb_lines, boot, hqi_masked.join(", "), mb_gather, metavm,
			hqi_unmask = hqi_unmask,
			ak_gate = ak_gate,
			env_gate = env_gate,
			strlit = strlit,
			v_ls = v_ls, v_ts = v_ts, v_dbg = v_dbg, v_inf = v_inf,
			v_os = v_os, v_clock = v_clock,
			v_s = v_s,
			nlok_delta = nlok_delta_e,
			pseed = pseed_e, pstep = pstep_e, pkm = pkm_e, pkc = pkc_e,
			pblock = pblock_e,
			shcb1 = sh_cb1_e, shcb2 = sh_cb2_e, shcbc = sh_cbc_e,
			shcc1 = sh_cc1_e, shcc2 = sh_cc2_e, shccc = sh_ccc_e,
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
	// ㉗: v15 运行入口 = 运行时碎片装配出的不透明函数值 RUN
	let entry_run = if v15 { "RUN" } else { "run" };
	let entry_tail = if bind_key.is_some() {
		format!(
			"local _bt = {{ ... }}\n  local vargs = {{}}\n  for _bi = 2, #_bt do\n    vargs[_bi - 1] = _bt[_bi]\n  end\n  local V2 = {{}}\n  return {}({}, V2, {{}}, vargs, #vargs)",
			entry_run, entry_pf
		)
	} else {
		format!("local vargs = {{}}\n  local V2 = {{}}\n  return {}({}, V2, {{}}, vargs, 0)", entry_run, entry_pf)
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
	// ㉗ (R009/Suda 回合 — 运行时入密): R008/Suda 实测破解 = 三个文本探针
	// 全部插在**可见**运行时表面（entry 的 run(MPF,...)、可见 newE 内的
	// DDEC 调用点、可见蹦床的 CT 索引）。根治：把 newE/makefn/run+蹦床
	// +DDEC 整体装进加密运行时碎片（wire 208），可见层只剩不透明函数值
	// 交接——可见文本从此没有任何一行触碰解码数据。要提取必须回到的路径
	// = 破 HQ 碎片加密（五层引导链，每构建重建）。
	let run_block: String = if v15 {
		// ㉗: 运行时碎片（wire 208）已在 hfrag 段装入（newE/makefn/
		// run+蹦床+DDEC 全入密）；可见层只剩这一次不透明函数值装配。
		run_unpack = String::new();
		run_soa = String::new();
		// ㉚ B-2: 链终值 SH1/SH2 + 归零常量 RPK6..11 入密交给 RT 工厂。
		String::from(
			"local RUN = RTFRAG(RPK1, RPK2, RPK3, RPK4, RPK5, SALT, P, G, U, FLOOR, MS, AV, HW2, TP, mget, resolve_call, callcap, HAS_LEN_META, CT1, CT2, CT3, CT4, CX1, CX2, CX3, CX4, CLK, SH1, SH2, RPK6, RPK7, RPK8, RPK9, RPK10, RPK11)",
		)
	} else {
		let run_head = String::new();
		let pc_decl = String::from("local pc = 1\n    ");
		let ln_decl = "local lastn = 0\n    local lastbase = 0\n    ";
		let run_loop = format!("while true do\n      {}\n      {}\n    end", fetch, branches);
		// legacy: makefn 仍是可见声明（历史形态，未入密）
		format!(
			"local run\n  {mk}\n  run = function(pf, V, ups, vargs, vargc)\n    {run_head}{run_unpack}\n    {run_soa}\n    {pc_decl}{o_decl}{ln_decl}{handler_defs}{run_loop}\n  end",
			mk = makefn_decl,
			run_head = run_head,
			run_unpack = run_unpack,
			run_soa = run_soa,
			pc_decl = pc_decl,
			o_decl = String::new(),
			ln_decl = ln_decl,
			handler_defs = handler_defs,
			run_loop = run_loop,
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
  {run_block}
  {entry_tail}
end
"#,
	vm_params = vm_params,
	entry_tail = entry_tail,
	run_block = run_block,
	params = params,
		kf_block = kf_block,
		oc_table = oc_table,
		p_fill = p_fill,
		prim_unpack = prim_unpack,
		helpers = helpers,
		hub_decl = hub_decl,
		hfrag = hfrag,
		v15_selfmod = v15_selfmod,
		decode_seg = decode_seg,
		ck_consts = ck_consts,
		parse_fn = parse_fn_vis,
		rt_helpers = rt_helpers,
		ms_block = ms_block,
	)
}
