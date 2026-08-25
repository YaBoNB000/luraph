//! L6 VM — instruction set + bytecode encoding.
//!
//! Register VM. Bytecode layout per function (u16 = little-endian):
//!
//!   [nregs u16][nparams u16][vararg u8]
//!   [nups u16] + nups × [src u16]        -- upvalue = slot of the CREATING
//!                                        -- frame's value array (materialization
//!                                        -- invariant: the creating frame holds
//!                                        -- every upvalue symbol in its own V)
//!   [nconst u16] + const items:
//!     [type u8]  0=nil | 1=bool(1 byte) | 2=number(text) | 3=string
//!     number: [len u16][ASCII digits]  (tonumber round-trips exactly)
//!     string: [len u16][raw bytes]
//!   SoA code (M5):
//!     [ncode u16]
//!     [OC : ncode × u8]                  -- opcode array
//!     [S0 : ncode × 7-bit-varint]        -- operand stream 0
//!     [S1 : ncode × 7-bit-varint]
//!     [S2 : ncode × 7-bit-varint]
//!     [S3 : ncode × 7-bit-varint]
//!
//! 7-bit varint (v15 complete tier): 7/14/21-bit + 128-base rebuild.
//! Decoder needs only +,-,* (no bitops — 5.1 template constraint):
//!   b1 < 128 -> v = b1
//!   b2 < 128 -> v = (b1-128) + b2*128
//!   b3 < 128 -> v = (b1-128) + (b2-128)*128 + b3*16384
//!   else     -> v = (b1-128) + (b2-128)*128 + (b3-128)*16384 + b4*2097152
//! Values ≥ 2³¹ are folded via `v - 2³²` (unsigned→signed, v15 归一化).
//!
//! M5 hub randomization: the four operand STREAM SLOTS hold a,b,c,d in
//! a per-build random permutation (VmProgram.slot_perm); the template
//! maps stream positions back to a/b/c/d.
//!
//! Jump targets are 1-based INSTRUCTION indexes into the SoA arrays
//! (pc steps by 1; no byte-offset fixpoint).
//! Operands are 0-based register/const/function indexes; the runtime
//! adds 1 for Lua arrays.
//!
//! Opcodes are randomly permuted per build (VMC, seed-derived): the
//! compiler and the interpreter template share one mapping, so the
//! emitted interpreter has no recognizable opcode layout.
//!
//! Dead-instruction padding (M5): Nop instructions are injected at
//! seed-derived positions; the dispatch carries a harmless Nop handler.
//!
//! Bytecode carrier (M5): each function blob is wrapped in a per-build
//! base-95 encoding + 10-special → 5-char token escape (v15 pC 同款)
//! before being embedded as a string literal.

/// Base opcode table (pre-permutation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
	Jmp,
	Jf,
	Jt,
	LoadNil,
	LoadK,
	Move,
	Add,
	Sub,
	Mul,
	Div,
	Mod,
	Pow,
	Concat,
	Unm,
	Not,
	Len,
	Lt,
	Le,
	Gt,
	Ge,
	Eq,
	Ne,
	Idiv,
	NewTab,
	GetTab,
	SetTab,
	TabN,
	CallT,
	Closure,
	Call,
	VarArgTab,
	VarArgC,
	VarArgTabN,
	GetGlobal,
	SetGlobal,
	GetUp,
	SetUp,
	Return,
	Nop,
	/// Call with FULL-EXPANSION results: f at V[a+1], b fixed args at
	/// V[a+2..a+1+b]; when d >= 2 a tail follows — `V[a+2+b]` holds the
	/// tail result count (stored by the nested CallE) and the tail values
	/// at V[a+3+b..]. d&1 = append varargs. Results → V[a+2..] and the
	/// RESULT COUNT is stored in the function slot V[a+1].
	CallE,
	/// Call with a variable-length tail (normal result handling): f at
	/// V[a+1], b fixed args at V[a+2..a+1+b], `V[a+2+b]` = tail count
	/// (stored by a preceding CallE), tail values at V[a+3+b..]; d =
	/// append varargs. c = nres (255 = all).
	CallM,
}

pub const N_OPS: usize = 41;

pub fn op_base() -> [u8; N_OPS] {
	#[allow(clippy::declare_interior_mutable_const)]
	#[rustfmt::skip]
	let t: [(Op, u8); N_OPS] = [
		(Op::Jmp, 0), (Op::Jf, 1), (Op::Jt, 2), (Op::LoadNil, 3),
		(Op::LoadK, 4), (Op::Move, 5), (Op::Add, 6), (Op::Sub, 7),
		(Op::Mul, 8), (Op::Div, 9), (Op::Mod, 10), (Op::Pow, 11),
		(Op::Concat, 12), (Op::Unm, 13), (Op::Not, 14), (Op::Len, 15),
		(Op::Lt, 16), (Op::Le, 17), (Op::Gt, 18), (Op::Ge, 19),
		(Op::Eq, 20), (Op::Ne, 21), (Op::Idiv, 22), (Op::NewTab, 23),
		(Op::GetTab, 24), (Op::SetTab, 25), (Op::TabN, 26), (Op::CallT, 27),
		(Op::Closure, 28), (Op::Call, 29), (Op::VarArgTab, 30), (Op::VarArgC, 31),
		(Op::VarArgTabN, 32), (Op::GetGlobal, 33), (Op::SetGlobal, 34),
		(Op::GetUp, 35), (Op::SetUp, 36), (Op::Return, 37), (Op::Nop, 38),
		(Op::CallE, 39), (Op::CallM, 40),
	];
	let mut out = [0u8; N_OPS];
	for (op, code) in t {
		// map Op -> its base code position
		let idx = op_index(op);
		out[idx] = code;
	}
	out
}

/// Op discriminant (stable order used for permutation).
pub fn op_index(op: Op) -> usize {
	match op {
		Op::Jmp => 0,
		Op::Jf => 1,
		Op::Jt => 2,
		Op::LoadNil => 3,
		Op::LoadK => 4,
		Op::Move => 5,
		Op::Add => 6,
		Op::Sub => 7,
		Op::Mul => 8,
		Op::Div => 9,
		Op::Mod => 10,
		Op::Pow => 11,
		Op::Concat => 12,
		Op::Unm => 13,
		Op::Not => 14,
		Op::Len => 15,
		Op::Lt => 16,
		Op::Le => 17,
		Op::Gt => 18,
		Op::Ge => 19,
		Op::Eq => 20,
		Op::Ne => 21,
		Op::Idiv => 22,
		Op::NewTab => 23,
		Op::GetTab => 24,
		Op::SetTab => 25,
		Op::TabN => 26,
		Op::CallT => 27,
		Op::Closure => 28,
		Op::Call => 29,
		Op::VarArgTab => 30,
		Op::VarArgC => 31,
		Op::VarArgTabN => 32,
		Op::GetGlobal => 33,
		Op::SetGlobal => 34,
		Op::GetUp => 35,
		Op::SetUp => 36,
		Op::Return => 37,
		Op::Nop => 38,
		Op::CallE => 39,
		Op::CallM => 40,
	}
}

/// Per-build opcode permutation: `mapping[op_index]` = the wire code.
#[derive(Debug, Clone)]
pub struct OpMap {
	/// wire code for each base op (op_index order)
	pub to_wire: [u8; N_OPS],
}

impl OpMap {
	pub fn new(rng: &mut crate::rng::Rng) -> OpMap {
		let mut to_wire: Vec<u8> = (0..N_OPS as u8).collect();
		// Fisher-Yates via the project rng
		for i in (1..to_wire.len()).rev() {
			let j = rng.int(0, i as i64) as usize;
			to_wire.swap(i, j);
		}
		OpMap {
			to_wire: to_wire.try_into().unwrap(),
		}
	}

	pub fn code(&self, op: Op) -> u8 {
		self.to_wire[op_index(op)]
	}

	/// `op == code` test expression text for the interpreter template.
	pub fn codes_text(&self) -> String {
		// "OPCODES[1], OPCODES[2], ..." in op_index order (template names
		// them OP_<name> in this same order)
		let mut s = String::new();
		for i in 0..N_OPS {
			if i > 0 {
				s.push(',');
			}
			s.push_str(&self.to_wire[i].to_string());
		}
		s
	}
}

// ---------------------------------------------------------------------------
// instruction encoding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Instr {
	pub op: Op,
	pub a: u16,
	pub b: u16,
	pub c: u16,
	pub d: u16,
}

impl Instr {
	pub fn new(op: Op, a: u16, b: u16, c: u16, d: u16) -> Instr {
		Instr { op, a, b, c, d }
	}
	pub fn op(op: Op) -> Instr {
		Instr {
			op,
			a: 0,
			b: 0,
			c: 0,
			d: 0,
		}
	}
	pub fn ab(op: Op, a: u16, b: u16) -> Instr {
		Instr {
			op,
			a,
			b,
			c: 0,
			d: 0,
		}
	}
	pub fn abc(op: Op, a: u16, b: u16, c: u16) -> Instr {
		Instr {
			op,
			a,
			b,
			c,
			d: 0,
		}
	}
	pub fn abcd(op: Op, a: u16, b: u16, c: u16, d: u16) -> Instr {
		Instr { op, a, b, c, d }
	}
	pub fn operands(&self) -> [u16; 4] {
		[self.a, self.b, self.c, self.d]
	}
}

pub fn push_u16(out: &mut Vec<u8>, v: u16) {
	out.push((v & 0xff) as u8);
	out.push((v >> 8) as u8);
}

/// 7/14/21-bit varint (v15 complete tier). Continuation = byte ≥ 128.
/// Width: v<128 → 7-bit; v<16384 → 14-bit; v<2097152 → 21-bit; else 28-bit.
pub fn encode_varint(out: &mut Vec<u8>, v: u32) {
	if v < 128 {
		out.push(v as u8);
	} else if v < 16_384 {
		out.push(((v % 128) + 128) as u8);
		out.push((v / 128) as u8);
	} else if v < 2_097_152 {
		out.push(((v % 128) + 128) as u8);
		out.push((((v / 128) % 128) + 128) as u8);
		out.push((v / 16_384) as u8);
	} else {
		out.push(((v % 128) + 128) as u8);
		out.push((((v / 128) % 128) + 128) as u8);
		out.push((((v / 16_384) % 128) + 128) as u8);
		out.push((v / 2_097_152) as u8);
	}
}

pub fn encode_u16var(out: &mut Vec<u8>, v: u16) {
	encode_varint(out, v as u32);
}

/// Decode one varint (Rust-side; mirrors the 5.1-safe Lua decoder).
pub fn decode_varint(src: &[u8], p: usize) -> Option<(u32, usize)> {
	let b1 = *src.get(p)? as u32;
	if b1 < 128 {
		return Some((b1, p + 1));
	}
	let b2 = *src.get(p + 1)? as u32;
	if b2 < 128 {
		return Some(((b1 - 128) + b2 * 128, p + 2));
	}
	let b3 = *src.get(p + 2)? as u32;
	if b3 < 128 {
		let v = (b1 - 128) + (b2 - 128) * 128 + b3 * 16_384;
		return Some((v, p + 3));
	}
	let b4 = *src.get(p + 3)? as u32;
	let v = (b1 - 128) + (b2 - 128) * 128 + (b3 - 128) * 16_384 + b4 * 2_097_152;
	Some((v, p + 4))
}

/// SoA instruction stream: opcode array, then four parallel operand streams.
/// `slot_perm[i]` = which operand (0=a 1=b 2=c 3=d) occupies stream i.
pub fn encode_soa(code: &[Instr], map: &OpMap, slot_perm: &[u8; 4], out: &mut Vec<u8>) {
	push_u16(out, code.len() as u16);
	for ins in code {
		out.push(map.code(ins.op));
	}
	for sl in 0..4 {
		let which = slot_perm[sl] as usize;
		for ins in code {
			encode_u16var(out, ins.operands()[which]);
		}
	}
}

// ---------------------------------------------------------------------------
// constants
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Const {
	Nil,
	Bool(bool),
	Num(f64),
	Str(Vec<u8>),
}

impl Const {
	pub fn encode(&self, out: &mut Vec<u8>) {
		match self {
			Const::Nil => out.push(0),
			Const::Bool(b) => {
				out.push(1);
				out.push(*b as u8);
			}
			Const::Num(v) => {
				out.push(2);
				let text = num_text(*v);
				push_u16(out, text.len() as u16);
				out.extend_from_slice(text.as_bytes());
			}
			Const::Str(b) => {
				out.push(3);
				push_u16(out, b.len() as u16);
				out.extend_from_slice(b);
			}
		}
	}
}

// ---------------------------------------------------------------------------
// bytecode carrier: base-95 + 10-special token escape (v15 pC)
// ---------------------------------------------------------------------------

/// Specials that get replaced by 5-char tokens (v15 pC set).
pub const CARRIER_SPECIALS: [u8; 10] = [
	b'"', b'\'', b'%', b' ', b'$', b'!', b'~', b'#', b'}', b'&',
];

/// Per-build printable carrier wrapping a raw function blob.
///
/// Base-94 over a shuffled 32..126 minus one reserved glyph. The
/// reserved glyph prefixes every 5-char token, so tokens can never
/// collide with a digit-stream substring (v15 pC, collision-free).
#[derive(Debug, Clone)]
pub struct Carrier {
	/// Shuffled printable glyphs minus `reserved` — digit 0..93.
	pub alphabet: [u8; 94],
	/// Glyph that never appears in the digit stream; token[0].
	pub reserved: u8,
	/// 5-byte tokens (`reserved` + 4 alnum), one per CARRIER_SPECIALS.
	pub tokens: [String; 10],
}

impl Carrier {
	pub fn new(rng: &mut crate::rng::Rng) -> Carrier {
		let mut glyphs: Vec<u8> = (32u8..=126).collect();
		rng.shuffle(&mut glyphs);
		let reserved = glyphs[0];
		let alphabet: [u8; 94] = glyphs[1..].try_into().unwrap();
		let mut tokens = dummy_tokens();
		for i in 0..10 {
			loop {
				let t = gen_token(rng, reserved);
				if tokens[..i].iter().any(|x| x == &t) {
					continue;
				}
				tokens[i] = t;
				break;
			}
		}
		Carrier {
			alphabet,
			reserved,
			tokens,
		}
	}

	/// Encode raw bytes → base-94 groups of 5, then token-escape specials.
	/// A 4-byte little-endian length prefix is prepended so the decoder
	/// can drop padding. 94^5 > 2^32, so every u32 group is unique.
	pub fn encode(&self, data: &[u8]) -> Vec<u8> {
		let n = data.len() as u32;
		let mut src = Vec::with_capacity(data.len() + 8);
		src.extend_from_slice(&n.to_le_bytes());
		src.extend_from_slice(data);
		while src.len() % 4 != 0 {
			src.push(0);
		}
		let mut digits = Vec::with_capacity(src.len() / 4 * 5);
		for chunk in src.chunks(4) {
			let mut v = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
			let mut d = [0u8; 5];
			for i in 0..5 {
				d[4 - i] = self.alphabet[(v % 94) as usize];
				v /= 94;
			}
			digits.extend_from_slice(&d);
		}
		let mut out = Vec::with_capacity(digits.len() + 32);
		for &b in &digits {
			if let Some(idx) = CARRIER_SPECIALS.iter().position(|&s| s == b) {
				out.extend_from_slice(self.tokens[idx].as_bytes());
			} else {
				out.push(b);
			}
		}
		out
	}

	/// Rust-side inverse (unit tests + dbg). Mirrors the Lua decoder.
	pub fn decode(&self, enc: &[u8]) -> Option<Vec<u8>> {
		let mut s = Vec::with_capacity(enc.len());
		let mut p = 0;
		while p < enc.len() {
			if enc[p] == self.reserved {
				if p + 5 > enc.len() {
					return None;
				}
				let tok = &enc[p..p + 5];
				let mut hit = None;
				for i in 0..10 {
					if self.tokens[i].as_bytes() == tok {
						hit = Some(CARRIER_SPECIALS[i]);
						break;
					}
				}
				s.push(hit?);
				p += 5;
			} else {
				s.push(enc[p]);
				p += 1;
			}
		}
		if s.len() % 5 != 0 {
			return None;
		}
		let mut inv = [255u8; 256];
		for (i, &ch) in self.alphabet.iter().enumerate() {
			inv[ch as usize] = i as u8;
		}
		let mut raw = Vec::with_capacity(s.len() / 5 * 4);
		for chunk in s.chunks(5) {
			let mut v: u32 = 0;
			for &ch in chunk {
				let d = inv[ch as usize];
				if d == 255 {
					return None;
				}
				v = v.checked_mul(94)?.checked_add(d as u32)?;
			}
			raw.extend_from_slice(&v.to_le_bytes());
		}
		if raw.len() < 4 {
			return None;
		}
		let n = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
		if raw.len() < 4 + n {
			return None;
		}
		Some(raw[4..4 + n].to_vec())
	}
}

fn dummy_tokens() -> [String; 10] {
	std::array::from_fn(|i| format!("Tk{i:03}"))
}

fn gen_token(rng: &mut crate::rng::Rng, reserved: u8) -> String {
	const C: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
	let mut s = String::with_capacity(5);
	s.push(reserved as char);
	for _ in 0..4 {
		s.push(C[rng.int(0, (C.len() - 1) as i64) as usize] as char);
	}
	s
}

/// Number text that tonumber() round-trips exactly (same forms the
/// printer emits: integer -> decimal, float -> Rust {:?}).
pub fn num_text(v: f64) -> String {
	if v.is_nan() {
		// 5.1: NaN prints as "nan"; the corpus does not use NaN constants
		return "0.0/0.0".to_string();
	}
	if v.is_infinite() {
		return if v < 0.0 {
			"-math.huge".to_string()
		} else {
			"math.huge".to_string()
		};
	}
	if v.fract() == 0.0 && v.abs() < 1e15 {
		format!("{}", v as i64)
	} else {
		format!("{:?}", v)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn varint_roundtrip_tiers() {
		let samples = [
			0u32, 1, 127, 128, 255, 16_383, 16_384, 65_535, 100_000, 2_097_151,
			2_097_152, 10_000_000,
		];
		for &v in &samples {
			let mut o = Vec::new();
			encode_varint(&mut o, v);
			let (got, n) = decode_varint(&o, 0).unwrap();
			assert_eq!(got, v, "v={v} bytes={o:?}");
			assert_eq!(n, o.len());
			let expect_len = if v < 128 {
				1
			} else if v < 16_384 {
				2
			} else if v < 2_097_152 {
				3
			} else {
				4
			};
			assert_eq!(o.len(), expect_len, "tier len v={v}");
		}
	}

	#[test]
	fn carrier_roundtrip_many() {
		let mut rng = crate::rng::Rng::new(42);
		let c = Carrier::new(&mut rng);
		for &msg in &[
			&b""[..],
			b"a",
			b"hello",
			&[0u8, 1, 2, 255, 128, 10, 13][..],
			&(0u8..=255).collect::<Vec<u8>>(),
		] {
			let enc = c.encode(msg);
			assert!(
				!enc.contains(&c.reserved) || enc.windows(5).any(|w| w[0] == c.reserved),
				"reserved only as token prefix"
			);
			assert_eq!(c.decode(&enc).as_deref(), Some(msg));
		}
		// per-build uniqueness: a different seed must produce a different wrap
		let mut rng2 = crate::rng::Rng::new(7);
		let c2 = Carrier::new(&mut rng2);
		let a = c.encode(b"payload-xyz");
		let b = c2.encode(b"payload-xyz");
		assert_ne!(a, b);
	}
}
