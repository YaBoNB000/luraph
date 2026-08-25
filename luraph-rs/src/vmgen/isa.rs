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
//!   code: [op u8][4 x u16-7-bit-varint] per instruction (M5 7-bit tier)
//!
//! u16-7-bit-varint (v15-style 7-bit chunking): values < 128 encode in
//! 1 byte; otherwise 2 bytes = (low7 | 0x80) then high9. Variable
//! length destroys the fixed-stride (old 9-byte AoS) signature; the
//! decoder needs only +,-,* (no bitops — 5.1 template constraint):
//!   b1 < 128 -> v = b1        else v = (b1 - 128) + b2 * 128
//!
//! M5 hub randomization: the four operand STREAM SLOTS hold a,b,c,d in
//! a per-build random permutation (VmProgram.slot_perm); the template
//! maps stream positions back to a/b/c/d.
//!
//! Jump targets are 1-based BYTE offsets into the code stream (computed
//! from per-instruction encoded lengths at resolve time).
//! Operands are 0-based register/const/function indexes; the runtime
//! adds 1 for Lua arrays.
//!
//! Opcodes are randomly permuted per build (VMC, seed-derived): the
//! compiler and the interpreter template share one mapping, so the
//! emitted interpreter has no recognizable opcode layout.
//!
//! Dead-instruction padding (M5): Nop instructions are injected at
//! seed-derived positions; the dispatch carries a harmless Nop handler.

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
	/// `slot_perm[i]` = which operand (0=a 1=b 2=c 3=d) sits in stream
	/// slot i (M5 per-build hub randomization).
	pub fn encode(&self, map: &OpMap, slot_perm: &[u8; 4], out: &mut Vec<u8>) {
		out.push(map.code(self.op));
		let ops = [self.a, self.b, self.c, self.d];
		for &sl in slot_perm {
			encode_u16var(out, ops[sl as usize]);
		}
	}
}

pub fn push_u16(out: &mut Vec<u8>, v: u16) {
	out.push((v & 0xff) as u8);
	out.push((v >> 8) as u8);
}

/// 7-bit-chunk varint for u16: 1 byte when < 128, else 2 bytes
/// ((v & 0x7F) | 0x80, v >> 7). Bitop-free decoder (5.1-safe):
/// b1 < 128 -> v = b1;  else v = (b1 - 128) + b2 * 128.
pub fn encode_u16var(out: &mut Vec<u8>, v: u16) {
	if v < 128 {
		out.push(v as u8);
	} else {
		out.push(((v & 0x7f) | 0x80) as u8);
		out.push((v >> 7) as u8);
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
