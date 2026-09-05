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
use crate::vmgen::isa::{Carrier, OpMap, CARRIER_SPECIALS, N_OPS};
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

/// 增量⑩ (防静态, 报告突破口 #5/#2): key material is never emitted as a
/// literal. The R001 break came from reading the keys straight out of
/// the output (seed literal next to the number table). Now every 28-bit
/// key K is assembled AT RUNTIME from random fragments parked in the KF
/// table; a per-key recipe is drawn from three shapes:
///
///   additive `(KF[a] + KF[b]) % 2^28`
///   affine   `(KF[a] * KF[b] + KF[c]) % 2^28`
///   anchored `(KF[a] + KF[b] * <anchor>) % 2^28`
///            (anchor = a boot-time table length, e.g. #APH / #hqi —
///             a value that exists only after boot code ran)
///
/// Fragment values are individually random and meaningless; both the
/// stored values and the slot indexes go through obf_num, so literal
/// scanning recovers no complete key. All arithmetic stays exact in
/// doubles (products bounded < 2^45).
const KEY_MOD: i64 = 268_435_456; // 2^28

struct KeyEmitter {
	slots: Vec<i64>,
	si: usize,
	writes: Vec<String>,
}

impl KeyEmitter {
	fn new(rng: &mut Rng) -> KeyEmitter {
		let mut slots: Vec<i64> = (1..=400).collect();
		rng.shuffle(&mut slots);
		KeyEmitter { slots, si: 0, writes: Vec::new() }
	}
	/// Park one fragment: obfuscated value at an obfuscated slot index.
	/// Returns the slot number (the recipe re-obfuscates its own index).
	fn frag(&mut self, v: i64, rng: &mut Rng) -> i64 {
		let slot = self.slots[self.si];
		self.si += 1;
		self.writes.push(format!(
			"KF[{}] = {}",
			obf_num(slot as u64, rng),
			obf_num(v as u64, rng)
		));
		slot
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
		let idx = |v: i64, rng: &mut Rng| obf_num(v as u64, rng);
		let form = if anchor.is_some() { rng.int(0, 2) } else { rng.int(0, 1) };
		match form {
			0 => {
				// additive: K = (A + B) % M
				let a = rng.int(0, KEY_MOD - 1);
				let b = (key - a).rem_euclid(KEY_MOD);
				let sa = self.frag(a, rng);
				let sb = self.frag(b, rng);
				format!(
					"((KF[{}] + KF[{}]) % {})",
					idx(sa, rng),
					idx(sb, rng),
					KEY_MOD
				)
			}
			1 => {
				// affine: K = (A*B + C) % M (B small odd, product exact)
				let b = rng.int(1, 16383) | 1;
				let a = rng.int(0, KEY_MOD - 1);
				let c = (key - a * b).rem_euclid(KEY_MOD);
				let sa = self.frag(a, rng);
				let sb = self.frag(b, rng);
				let sc = self.frag(c, rng);
				format!(
					"((KF[{}] * KF[{}] + KF[{}]) % {})",
					idx(sa, rng),
					idx(sb, rng),
					idx(sc, rng),
					KEY_MOD
				)
			}
			_ => {
				// anchored: K = (A + B*anchor) % M
				let (ae, av) = anchor.unwrap();
				let b = rng.int(1, 999);
				let a = (key - b * av).rem_euclid(KEY_MOD);
				let sa = self.frag(a, rng);
				let sb = self.frag(b, rng);
				format!(
					"((KF[{}] + KF[{}] * {}) % {})",
					idx(sa, rng),
					idx(sb, rng),
					ae,
					KEY_MOD
				)
			}
		}
	}
	/// `local KF = {}` + the (shuffled) fragment writes.
	fn block(mut self, rng: &mut Rng) -> String {
		rng.shuffle(&mut self.writes);
		format!("local KF = {{}}\n  {}\n  ", self.writes.join("\n  "))
	}
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
) -> String {
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
      local b = (BYTE(s, p) - cks % 256) % 256
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
    local g = (BSEED + (fi - 1) * BSTEP) % 268435456
    local um = {}
    for i = 1, #s do
      g = (BKM * g + BKC) % 268435456
      um[i] = CHAR((BYTE(s, i) - g % 256) % 256)
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
		// stage E3: per-prototype operand-stream checksum table (a
		// build-time fold of every wire operand); parse re-reads the
		// streams through the inline ladders and re-folds -- a mismatch
		// means tamper -> silent trap (no os.clock, F18 safe)
		// 增量⑪: checksum values obfuscated (no bare literal oracle)
		let os_list = operand_sums
			.iter()
			.map(|s| obf_num(*s, rng))
			.collect::<Vec<_>>()
			.join(", ");
		format!(
			"local OS = {{{}}}\n  local di = 1\n  while di <= #FN do\n    PF[di] = parse(FN[di], di)\n    if PF[di].ck ~= OS[di] then while true do end end\n    di = di + 1\n  end",
			os_list
		)
	} else {
		String::from("for i = 1, #FN do PF[i] = parse(FN[i], i) end")
	};

	// v15 (P3-B): bytecode self-modification + dead dispatch segment.
	//   self-mod: the compiler reports every Nop site; at load time each
	//   is rewritten to the Nop-alias wire value via a LITERAL constant
	//   write (sample `J[Q]=12` shape, fingerprints F14/F27). The
	//   primary Nop wire becomes dead (decoy), the alias carries every
	//   dead instruction, and opcode encoding is unstable within the
	//   stream. Both writes and semantics are neutral (Nop -> Nop).
	//   dead segment: a site1-shaped fetch tree (sample's never-hit
	//   decode path) guarded by an always-false flag.
	let v15_selfmod = if v15 {
		let mut sm = String::new();
		for (fi, sites) in nop_sites.iter().enumerate() {
			if sites.is_empty() {
				continue;
			}
			// bind the opcode array to a local and write through it
			// (sample shape: direct array constant writes, F14)
			sm.push_str(&format!("  local ZW{} = PF[{}].W\n", fi + 1, fi + 1));
			for &p in sites {
				sm.push_str(&format!(
					"  ZW{}[{}] = NOPA\n",
					fi + 1,
					p as usize + 1,
				));
			}
		}
		sm.push_str("  local DA = {}\n  local DP = 1\n  local DF = 0\n  while DF > 0 do\n    local f = DA[DP]\n    if f >= 4 then\n      if f < 6 then\n        if f ~= 5 then DF = 0 else DF = 0 end\n      else DF = 0 end\n    elseif f < 2 then DF = 0\n    else DF = 0 end\n");
		// P3c: decoy fetch points — the sample ships several dispatch
		// loops (golden F11 = 19); these dead fetch shapes raise the
		// family resemblance and multiply the "which loop is real"
		// question for an analyst. Never executed (DF stays 0).
		let n_decoys = rng.int(1, 3);
		for _ in 0..n_decoys {
			let i1 = rng.int(0, (N_OPS - 1) as i64);
			let i2 = rng.int(0, (N_OPS - 1) as i64);
			sm.push_str(&format!(
				"    local oc = DA[DP]\n    if oc then\n      local a = DA[DP]\n      local b = DA[DP]\n      local c = DA[DP]\n      local d = DA[DP]\n      DP = DP + 1\n      if oc == OCt[{}] then DA[DP] = a elseif oc == OCt[{}] then DA[DP] = b else local r = DA[DP](a, b, c, d); if r then DF = r[1] end end\n    end\n",
				i1, i2
			));
		}
		sm.push_str("    DP = DP + 1\n  end\n");
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
	let run_unpack = format!(
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
			ch.wrapping_add((astate % 256) as u8).to_string()
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
		"local APH = {{{}}}\n  local AL = {{}}\n  do\n    local ast = {}\n    for i = 1, 94 do\n      ast = ({} * ast + {}) % 268435456\n      AL[(APH[i] - ast % 256) % 256] = i - 1\n    end\n  end\n",
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
			b.wrapping_add((tkstate % 256) as u8).to_string()
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
		"local TKD = {{{}}}\n  local TK = {{}}\n  do\n    local tst = {}\n    local tb = {{}}\n    for i = 1, 60 do\n      tst = ({} * tst + {}) % 268435456\n      tb[i] = (TKD[i] - tst % 256) % 256\n    end\n    for i = 0, 9 do\n      TK[CHAR(tb[i * 5 + 1], tb[i * 5 + 2], tb[i * 5 + 3], tb[i * 5 + 4], tb[i * 5 + 5])] = CHAR(tb[50 + i + 1])\n    end\n  end\n",
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

	// makefn: v15 stage A variant translates upvalue descriptors and
	// parameter fills through the scattered slot tables (the PARENT's
	// S resolves upsrc register references; the child's pf.S maps the
	// parameter registers). Legacy keeps the dense layout.
	let makefn_decl = if v15 {
		String::from(
			"local function makefn(idx, V, upsf, S)
    local pf = PF[idx]
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
      -- real TCO: tail call into run so deep tail recursion reuses the
      -- frame instead of stacking one per level
      return run(pf, V2, c, vargs, vargc)
    end
  end",
		)
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
	// Jump handlers return `{j = target}` signals (pc is the loop's
	// local); Call/CallE/CallM/CallT stay inline in the CPS chain.
	let env_names = [
		"V", "C", "S", "O", "G", "vargs", "vargc", "ups", "makefn", "mget",
		"resolve_call", "callcap", "HAS_LEN_META", "CHAR", "FLOOR", "ERR",
		"TYP", "GMT", "RGET", "RSET", "U", "MS",
	];
	let prelude_lhs = env_names.join(", ");
	let prelude_rhs: Vec<String> =
		(0..env_names.len()).map(|i| format!("E[{}]", i + 1)).collect();
	let prelude_rhs = prelude_rhs.join(", ");
	let e_ctor = format!(
		"local E = {{{}, ln = 0, lb = 0}}",
		env_names.join(", ")
	);
	let hfrag: String;
	let handler_defs = if v15 {
		let hm = (rng.int(1_048_577, 33_000_001) | 1) as u32;
		let hc = (rng.int(1_048_576, 268_000_000) | 1) as u32;
		let hseed = rng.int(1_048_576, 268_435_455) as u32;
		let mut frags: Vec<(u8, Vec<u8>)> = Vec::new(); // (wire, source bytes)
		for (name, wire) in &items {
			if matches!(name.as_str(), "Call" | "CallE" | "CallM" | "CallT") {
				continue; // inline in the CPS chain, never via HW
			}
			let mut body = if name == "Nop" || name == "NopA" {
				nop.clone()
			} else {
				handlers::gen(name, fmt_of[name], true, &mut pool, mk)
			};
			if name == "Return" {
				// CPS signal form; frame bookkeeping moves into E
				body = body
					.replace("return U(out, 1, total)", "return {out, total}");
			}
			body = body.replace("lastbase", "E.lb").replace("lastn", "E.ln");
			if matches!(name.as_str(), "Jmp" | "Jf" | "Jt") {
				body = body.replace("pc = b", "return {j = b}");
			}
			let src = format!(
				"return function(E,a,b,c,d) local {} = {} {} end",
				prelude_lhs, prelude_rhs, body
			);
			frags.push((*wire, src.into_bytes()));
		}
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
					b.wrapping_add((state % 256) as u8)
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
		let mut boot = String::new();
		let v_ls = "hls";
		let v_dbg = "hdbg";
		let v_inf = "hinf";
		let v_s = "hsarg";
		let v_c = "hcb";
		boot.push_str(&coded_name_tpl(rng, v_ls, "loadstring"));
		boot.push_str(&coded_name_tpl(rng, v_dbg, "debug"));
		boot.push_str(&coded_name_tpl(rng, v_inf, "info"));
		boot.push_str(&coded_name_tpl(rng, v_s, "s"));
		boot.push_str(&coded_name_tpl(rng, v_c, "[C]"));
		let build = format!(
			r#"{}  local HW = {{}}
  do
    {}local LS = GFE(0)[{v_ls}]
    local DBG = GFE(0)[{v_dbg}]
    local INF = DBG and DBG[{v_inf}]
    local nlok = false
    do
      local ok, sr = PCAL(function() return INF(LS, {v_s}) end)
      if ok and sr == {v_c} then nlok = true end
    end
    if not nlok then while true do end end
    local hqi = {{{}}}
    local hi = 1
    while hi <= #hqi do
      local w = hqi[hi]
      local seg = HQ[hqi[hi + 1]]
      local flen = hqi[hi + 2]
      hi = hi + 3
      local hs = ({} + w * {}) % 268435456
      local t = {{}}
      local ti = 1
      local n = #seg
      for i = 1, n, 5 do
        local v = 0
        v = v * 94 + AL[BYTE(seg, i)]
        v = v * 94 + AL[BYTE(seg, i + 1)]
        v = v * 94 + AL[BYTE(seg, i + 2)]
        v = v * 94 + AL[BYTE(seg, i + 3)]
        v = v * 94 + AL[BYTE(seg, i + 4)]
        local b1 = v % 256; v = FLR(v / 256)
        local b2 = v % 256; v = FLR(v / 256)
        local b3 = v % 256; v = FLR(v / 256)
        local b4 = v % 256
        hs = ({} * hs + {}) % 268435456; t[ti] = CHAR((b1 - hs % 256) % 256); ti = ti + 1
        hs = ({} * hs + {}) % 268435456; t[ti] = CHAR((b2 - hs % 256) % 256); ti = ti + 1
        hs = ({} * hs + {}) % 268435456; t[ti] = CHAR((b3 - hs % 256) % 256); ti = ti + 1
        hs = ({} * hs + {}) % 268435456; t[ti] = CHAR((b4 - hs % 256) % 256); ti = ti + 1
      end
      HW[w] = LS(SUB(table.concat(t), 1, flen))()
    end
  end"#,
			hq_lines, boot, hqi.join(", "), hseed_e, hstep_e, hm_e, hc_e, hm_e, hc_e, hm_e, hc_e, hm_e, hc_e,
			v_ls = v_ls, v_dbg = v_dbg, v_inf = v_inf, v_s = v_s, v_c = v_c,
		);
		hfrag = build;
		String::new()
	} else {
		hfrag = String::new();
		String::new()
	};
	let (fetch, branches) = if v15 {
		// Inline the Call-family opcodes (Call/CallE/CallM/CallT) directly in
		// the CPS loop instead of dispatching them through H[]. Real TCO makes
		// the closure->run call a tail call, but CPS would still add an H[Call]
		// frame per call; inlining removes that frame so deep tail recursion
		// does not overflow the stack. Non-call opcodes still dispatch via H.
		let sa = stream_names[operand_stream[0] as usize];
		let sb = stream_names[operand_stream[1] as usize];
		let sc = stream_names[operand_stream[2] as usize];
		let sd = stream_names[operand_stream[3] as usize];
		// F11 fetch shape: `local oc=W[pc];if oc then ... end`.
		// 建议1: the inline call-family chain order is drawn per build
		// (equality tests on unique wire codes — order is neutral).
		let mut chain: Vec<&str> = vec!["Call", "CallE", "CallM", "CallT"];
		rng.shuffle(&mut chain);
		let mut cond = String::new();
		for (i, name) in chain.iter().enumerate() {
			if i > 0 {
				cond.push_str(" elseif ");
			}
			// P3a: frame bookkeeping moves into the per-frame env E so
			// the (encrypted) Return handler can read it back
			let body = handlers::gen(name, fmt_of[*name], true, &mut pool, mk)
				.replace("lastbase = a + 1", "E.lb = a + 1")
				.replace("lastn = nout", "E.ln = nout");
			let idx = OP_NAMES.iter().position(|n| n == name).unwrap();
			cond.push_str(&format!("oc == OCt[{}] then {}", idx, body));
		}
		let cps_fetch = format!(
			"local oc = W[pc];if oc then local a = {sa}[pc];local b = {sb}[pc];local c = {sc}[pc];local d = {sd}[pc];pc = pc + 1;if {cond} else local r = HW[oc](E,a,b,c,d); if r then if r.j then pc = r.j else return U(r[1], 1, r[2]) end end end end",
			sa = sa, sb = sb, sc = sc, sd = sd, cond = cond,
		);
		(cps_fetch, String::new())
	} else {
		(fetch, branches)
	};

	// runtime helpers with pool-routed literals (resolve_call's type/
	// meta names, callcap's '#' vararg selector)
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
  local function callcap(f, args, nargs)
    local w = function(...)
      local t = {{ ... }}
      return t, SEL({hash}, ...)
    end
    return w(f(U(args, 1, nargs)))
  end",
		function = pool.lit("function"),
		call = pool.lit("__call"),
		table = pool.lit("table"),
		callmsg = pool.lit("attempt to call a "),
		value = pool.lit(" value"),
		hash = pool.lit("#"),
	);
	let ms_block = if v15 { pool.boot_block() } else { String::new() };

	// 增量⑩: the key-fragment table + all fragment writes land at the
	// very top of the VM body (before the first key use in oc_boot).
	let kf_block = ke.block(rng);
	format!(
		r#"local VM = function({params})
  {kf_block}{oc_table}
  local FN = {{{params}}}
  local PF = {{}}
  {p_fill}  {prim_unpack}
  {ms_block}{helpers}
  {ck_consts}{parse_fn}
  {decode_seg}
{v15_selfmod}{hfrag}
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
  local run
  {makefn_decl}
  run = function(pf, V, ups, vargs, vargc)
    {run_unpack}
    {run_soa}
    local pc = 1
    {o_decl}{ln_decl}{handler_defs}
    while true do
      {fetch}
      {branches}
    end
  end
  local vargs = {{}}
  local V2 = {{}}
  return run(PF[#FN], V2, {{}}, vargs, 0)
end
"#,
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
		ln_decl = if v15 {
			e_ctor
		} else {
			"local lastn = 0\n    local lastbase = 0".to_string()
		},
		fetch = fetch,
		branches = branches,
		v15_selfmod = v15_selfmod,
		decode_seg = decode_seg,
		ck_consts = ck_consts,
		parse_fn = parse_fn,
		makefn_decl = makefn_decl,
		rt_helpers = rt_helpers,
		ms_block = ms_block,
		o_decl = if v15 {
			// stage A: overflow table for nres=255 results spilling past
			// the scattered register allocation
			"local O = {}\n    ".to_string()
		} else {
			String::new()
		},
	)
}
