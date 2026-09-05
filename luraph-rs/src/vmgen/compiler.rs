//! L6 VM — compiler: AST → bytecode.
//!
//! One recursive walk compiles the whole program. Function indexes are
//! assigned post-order (a nested function is fully compiled before its
//! CLOSURE instruction is emitted in the parent). Registers are naive
//! (no liveness analysis in M4): every subexpression gets a fresh
//! register; loop variables use a per-iteration shared cell when
//! captured by a closure (Lua's fresh-variable-per-iteration capture
//! semantics) and a plain fixed register otherwise (no per-iteration
//! register growth).
//!
//! Multi-value rules:
//! - `local a, b = f()` / `a, b = f()`: the trailing call fills the
//!   remaining targets — the count is known at compile time, so the
//!   call uses a fixed nres into scratch regs, then stores. A trailing
//!   VARARG (`a, b = ...`) expands the same way through VarArgTab.
//! - `return e1, f()` / `return e1, ...`: truly variable — the CALL
//!   records (lastbase, lastn); RETURN(base, 255, c, pre) merges
//!   V[base+1..base+pre] + (c=0: V[lastbase+1..lastbase+lastn] |
//!   c=1: the varargs).
//! - table trailing call: CALLT stores results from the counter and
//!   advances it (variable, runtime).
//!
//! Upvalues (single-cell model, exact Lua 5.1 semantics):
//! - a cell = { v = <V array>, i = <slot> } — every closure that
//!   captures a symbol binds a (possibly aliased) reference to ONE
//!   cell, so reads/writes from any nesting level see one value
//! - plain descriptor (slot in the creating frame): cell =
//!   { v = V, i = slot }
//! - Slot descriptor (0x8000 | slot): per-iteration shared cell —
//!   V[slot] holds the current iteration's cell table `{ 1 = value }`;
//!   the interpreter binds `{ v = V[slot], i = 1 }`
//! - Up alias descriptor (0xC000 | upvalue index): the creating frame
//!   itself materializes the symbol as an upvalue — the closure
//!   aliases that frame's cell object directly (no intermediate value
//!   copies anywhere: materialization is a scope alias, not a copy)
//! - a function's upvalue list = symbols referenced by its own code or
//!   by any nested function that are declared in an enclosing function

use crate::ast::*;
use crate::rng::Rng;
use crate::symtab::SymTable;
use crate::vmgen::isa::{self, Carrier, Const, Instr, Op, OpMap};
use std::collections::{HashMap, HashSet};

/// How a symbol's value lives in the current frame.
/// - Plain(r): value is in register r (V[r]).
/// - Slot(sr): per-iteration shared cell — V[sr] holds a small table
///   `{ 1 = value }` recreated each loop iteration; every closure
///   created in the same iteration shares it, and body reads/writes go
///   through the same cell (Lua 5.1/Luau per-iteration local semantics).
/// - Up(u): a MATERIALIZED upvalue — the symbol's value lives in the
///   canonical cell of upvalue u (aliased, never copied): reads/writes
///   go straight through the shared cell, so every closure at every
///   nesting level sees one single cell (exact 5.1 semantics).
#[derive(Debug, Clone, Copy)]
enum CellKind {
	Plain(u16),
	Slot(u16),
	Up(u16),
}

/// v15 stage A: wire-operand role (see Ctx::op_roles).
#[derive(Debug, Clone, Copy, PartialEq)]
enum ORole {
	Val,
	Reg,
	Base,
	Const,
	Up,
}

pub struct VmProgram {
	pub opmap: OpMap,
	/// M5 hub randomization: operand stream-slot order.
	/// slot_perm[i] = which operand (0=a 1=b 2=c 3=d) occupies stream
	/// slot i. Encoder writes in this order; the template maps the
	/// stream positions back to a/b/c/d.
	pub slot_perm: [u8; 4],
	/// M5 bytecode carrier (base-94 + token escape). Shared by every
	/// function blob so the interpreter embeds one decoder.
	pub carrier: Carrier,
	pub fns: Vec<Vec<u8>>,
	/// Nop instruction indexes per function (0-based, parallel to
	/// `fns`). Consumed by the v15 template to emit literal-constant
	/// self-modification writes (sample `J[Q]=12` shape, F14/F27).
	pub nop_sites: Vec<Vec<u16>>,
	/// P1 (致命缺点③): constant-keystream LCG constants (mod 2^28),
	/// per build. The compiler masks every constant payload byte with
	/// the per-function seeded stream; the template parse mirrors it.
	pub ck_km: u32,
	pub ck_kc: u32,
	/// P1 (动态内联): MkStr per-position additive mask constants —
	/// mask(i) = (mk1 * i + mk2) % 65536 over the FINAL instruction
	/// index (post Nop-padding). Emitted into the MkStr handler.
	pub mk1: u16,
	pub mk2: u16,
	/// v15 stage E3: per-function sum of ALL wire operands (mod 2^32),
	/// parallel to `fns`. The v15 interpreter re-reads every operand
	/// stream with inline 7-bit ladders and folds the same checksum
	/// (F13 shape); a mismatch traps the decode loop.
	pub operand_sums: Vec<u64>,
}

#[derive(Debug, Clone, Copy)]
struct Label(u32);

struct Ctx<'a> {
	program: &'a mut VmProgram,
	rng: &'a mut Rng,
	symtab: &'a crate::symtab::SymTable,
	/// target dialect: true = Lua 5.1 (table-constructor store order:
	/// array part stores LAST), false = Luau (source order)
	lua51: bool,
	/// v15 stage A (operand scattering): registers/consts/upvalues are
	/// scattered into random slots per function before serialization
	scatter: bool,
	/// next nested-function slot (global DFS order over the chunk)
	next_fn_slot: usize,
	/// scope chain of declared syms -> cell kind (outermost first)
	scopes: Vec<HashMap<SymId, CellKind>>,
	/// upvalues of the current function: sym -> up idx
	upvals: HashMap<SymId, u16>,
	/// upvalue descriptors (creating frame's V slot, 1-based), up-idx order
	upsrc: Vec<u16>,
	next_reg: u16,
	nparams: u16,
	vararg: bool,
	consts: Vec<Const>,
	const_map: HashMap<(u8, u64), u16>,
	code: Vec<Instr>,
	/// label -> [positions]; entry 0 = label position (code len when
	/// here() ran), entries 1.. = jump-instruction slots (instr index + 1)
	labels: HashMap<u32, (Option<usize>, Vec<usize>)>,
	/// loop stack: (break_label, continue_label)
	loops: Vec<(Label, Label)>,
	/// active loop stack: for each enclosing loop being compiled, the
	/// set of loop-body-declared syms captured by a closure inside that
	/// loop body. A local declared in the body of the INNERMOST active
	/// loop gets a per-iteration shared cell (CellKind::Slot) iff it is
	/// in that set; otherwise a plain fixed register (reused each
	/// iteration — no per-iteration growth).
	cell_loops: Vec<HashSet<SymId>>,
}

/// Collision-free constant key: (tag, payload).
fn const_key(c: &Const) -> (u8, u64) {
	match c {
		Const::Nil => (0, 0),
		Const::Bool(b) => (1, *b as u64),
		Const::Num(v) => (2, v.to_bits()),
		Const::Str(b) => {
			let mut h: u64 = 0xcbf2_9ce4_8422_2325;
			for x in b {
				h ^= *x as u64;
				h = h.wrapping_mul(0x1_0000_0001_b3);
			}
			(3, h)
		}
	}
}

impl<'a> Ctx<'a> {
	fn new_main(
		program: &'a mut VmProgram,
		rng: &'a mut Rng,
		symtab: &'a crate::symtab::SymTable,
		lua51: bool,
		scatter: bool,
	) -> Ctx<'a> {
		Ctx {
			program,
			rng,
			symtab,
			lua51,
			scatter,
			next_fn_slot: 0,
			scopes: vec![HashMap::new()],
			upvals: HashMap::new(),
			upsrc: Vec::new(),
			next_reg: 0,
			nparams: 0,
			vararg: false,
			consts: Vec::new(),
			const_map: HashMap::new(),
			code: Vec::new(),
			labels: HashMap::new(),
			loops: Vec::new(),
			cell_loops: Vec::new(),
		}
	}

	fn tmp(&mut self) -> u16 {
		let r = self.next_reg;
		self.next_reg += 1;
		r
	}

	fn reserve(&mut self, n: u16) -> u16 {
		let r = self.next_reg;
		self.next_reg += n;
		r
	}

	fn kidx(&mut self, c: Const) -> u16 {
		let k = const_key(&c);
		if let Some(&i) = self.const_map.get(&k) {
			return i;
		}
		let i = self.consts.len() as u16;
		self.consts.push(c);
		self.const_map.insert(k, i);
		i
	}

	fn emit(&mut self, ins: Instr) -> usize {
		self.code.push(ins);
		self.code.len() - 1
	}

	fn new_label(&mut self) -> Label {
		Label(self.rng.int(1, 1_000_000_000) as u32)
	}

	fn here(&mut self, l: &Label) {
		let e = self
			.labels
			.entry(l.0)
			.or_insert_with(|| (None, Vec::new()));
		e.0 = Some(self.code.len());
	}

	fn jmp(&mut self, op: Op, reg: u16, l: &Label) {
		let pos = self.emit(Instr::ab(op, reg, 0));
		self.labels
			.entry(l.0)
			.or_insert_with(|| (None, Vec::new()))
			.1
			.push(pos + 1);
	}



	fn lookup(&self, s: SymId) -> Option<CellKind> {
		for sc in self.scopes.iter().rev() {
			if let Some(&k) = sc.get(&s) {
				return Some(k);
			}
		}
		None
	}

	fn declare(&mut self, s: SymId, k: CellKind) {
		self.scopes.last_mut().unwrap().insert(s, k);
	}

	/// The innermost active loop's capture set (a local declared now
	/// lives in the body of the innermost active loop, if any).
	fn cur_loop_captures(&self) -> Option<&HashSet<SymId>> {
		self.cell_loops.last()
	}

	fn push_scope(&mut self) {
		self.scopes.push(HashMap::new());
	}

	fn pop_scope(&mut self) {
		self.scopes.pop();
	}

	/// v15 stage A (operand scattering, luraph15 D1 family): operand
	/// roles per opcode.
	///   Reg   = single register — scattered (wire value = slot - 1)
	///   Base  = base of a CONTIGUOUS register range — kept logical;
	///           the v15 runtime translates through the per-function
	///           slot table S (LoadNil/Call*/Return iterate ranges)
	///   Const = constant index — scattered inside a padded pool
	///   Up    = upvalue index — scattered through the up permutation
	///   Val   = opaque value (jump target / count / flag / proto idx)
	fn op_roles(op: Op) -> [ORole; 4] {
		use ORole::*;
		match op {
			Op::Jmp => [Val, Val, Val, Val],
			Op::Jf | Op::Jt => [Reg, Val, Val, Val],
			Op::LoadNil => [Base, Val, Val, Val],
			Op::LoadK => [Reg, Const, Val, Val],
			Op::Move => [Reg, Reg, Val, Val],
			Op::Add
			| Op::Sub
			| Op::Mul
			| Op::Div
			| Op::Mod
			| Op::Pow
			| Op::Idiv
			| Op::Concat
			| Op::Lt
			| Op::Le
			| Op::Gt
			| Op::Ge
			| Op::Eq
			| Op::Ne
			| Op::GetTab
			| Op::SetTab
			| Op::TabN => [Reg, Reg, Reg, Val],
			Op::Unm | Op::Not | Op::Len => [Reg, Reg, Val, Val],
			Op::NewTab | Op::VarArgTab | Op::VarArgC => [Reg, Val, Val, Val],
			Op::VarArgTabN => [Reg, Reg, Val, Val],
			Op::CallT => [Base, Reg, Reg, Val],
			Op::Closure => [Reg, Val, Val, Val],
			Op::Call | Op::CallE | Op::CallM => [Base, Val, Val, Val],
			Op::GetGlobal | Op::SetGlobal => [Reg, Const, Val, Val],
			Op::GetUp => [Reg, Up, Val, Val],
			Op::SetUp => [Up, Reg, Val, Val],
			Op::Return => [Base, Val, Val, Val],
			Op::Nop => [Val, Val, Val, Val],
			// P1: a = destination register (scattered); b/c/d = masked
			// packed string bytes (opaque values, never scattered)
			Op::MkStr => [Reg, Val, Val, Val],
		}
	}

	/// v15 stage A: scatter registers/constants/upvalues into random
	/// slots and rewrite the wire operands accordingly. Returns the
	/// register slot table S (S[r] = 1-based physical slot of logical
	/// register r-1), serialized after the constants; the v15
	/// interpreter consults it for range-based accesses and makefn
	/// translates upvalue descriptors through the PARENT's S. After
	/// this pass register/const/up operands share one opaque numeric
	/// space — static register recovery loses its small-integer anchor.
	fn scatter_operands(&mut self, nregs: u16) -> Vec<u16> {
		// 1. register slots: sigma[r] = 1-based physical slot (~50%
		// density for small frames; extra spread capped at 64 so very
		// large frames stay in the 2-byte varint tier)
		let n = nregs as usize;
		let smax = n + (n / 2).min(64).max(8);
		let mut pool: Vec<u16> = (1..=smax as u16).collect();
		self.rng.shuffle(&mut pool);
		let sigma: Vec<u16> = pool[..n].to_vec();

		// 2. constant pool scattering: real constants land at random
		// positions in a padded pool; holes get nil / decoy numbers
		let nc = self.consts.len();
		let cmax = nc + (nc / 2).max(4);
		let mut cpos: Vec<usize> = (0..cmax).collect();
		self.rng.shuffle(&mut cpos);
		let kappa: Vec<usize> = cpos[..nc].to_vec();
		let mut consts2: Vec<Const> = vec![Const::Nil; cmax];
		let mut used = vec![false; cmax];
		for (i, c) in self.consts.drain(..).enumerate() {
			consts2[kappa[i]] = c;
			used[kappa[i]] = true;
		}
		for j in 0..cmax {
			if !used[j] && self.rng.int(0, 3) == 0 {
				consts2[j] = Const::Num(self.rng.int(0, 999_999) as f64);
			}
		}
		self.consts = consts2;

		// 3. upvalue permutation (upsrc reordered; operands map through
		// the inverse)
		let nu = self.upsrc.len();
		let mut perm: Vec<usize> = (0..nu).collect();
		self.rng.shuffle(&mut perm);
		let old_upsrc = std::mem::take(&mut self.upsrc);
		let mut tau = vec![0usize; nu];
		for (ni, &oi) in perm.iter().enumerate() {
			self.upsrc.push(old_upsrc[oi]);
			tau[oi] = ni;
		}

		// 4. wire operand rewrite
		for ins in self.code.iter_mut() {
			let roles = Self::op_roles(ins.op);
			let ops = [ins.a, ins.b, ins.c, ins.d];
			for k in 0..4 {
				let v = ops[k] as usize;
				let nv = match roles[k] {
					ORole::Reg => sigma[v] - 1,
					ORole::Const => kappa[v] as u16,
					ORole::Up => tau[v] as u16,
					_ => ops[k],
				};
				match k {
					0 => ins.a = nv,
					1 => ins.b = nv,
					2 => ins.c = nv,
					_ => ins.d = nv,
				}
			}
		}

		sigma
	}

	fn finish(mut self, nregs: u16) -> (Vec<u8>, Vec<u16>, u64) {
		// implicit trailing return (a chunk/function without an explicit
		// return returns nothing — the code must terminate)
		let needs_return = self
			.code
			.last()
			.map(|i| i.op != Op::Return)
			.unwrap_or(true);
		if needs_return {
			self.emit(Instr::abcd(Op::Return, 0, 0, 0, 0));
		}
		// M5 dead-instruction padding: Nops at seed-derived positions
		// (harmless in the dispatch; shifts label bookkeeping)
		let nops = if self.code.is_empty() {
			0
		} else {
			(self.code.len() / 10).max(1)
		};
		for _ in 0..nops {
			let pos = self.rng.int(0, self.code.len() as i64) as usize;
			self.code.insert(pos, Instr::op(Op::Nop));
			// label positions are instruction indexes; jump slots are
			// (instruction index + 1). An entry shifts iff its
			// instruction index >= pos, i.e. slot >= pos + 1.
			for (_, (at, jumps)) in self.labels.iter_mut() {
				if let Some(a) = at {
					if *a >= pos {
						*a += 1;
					}
				}
				for j in jumps.iter_mut() {
					if *j >= pos + 1 {
						*j += 1;
					}
				}
			}
		}
		// M5 SoA: jump targets are 1-based INSTRUCTION indexes into the
		// parallel arrays (pc steps by 1). No byte-offset fixpoint.
		for (_, (pos, jumps)) in &self.labels {
			if let Some(base) = pos {
				let target = (*base as u16) + 1;
				for &p in jumps {
					self.code[p - 1].b = target;
				}
			}
		}
		// P1 (动态内联): rewrite LoadK of SHORT strings (≤5 bytes) into
		// MkStr — the string never enters the constant pool; its bytes
		// ride the operand streams as masked immediates. Done BEFORE
		// scattering so operand a scatters as a register and the packed
		// b/c/d pass through as opaque values.
		for ins in self.code.iter_mut() {
			if ins.op == Op::LoadK {
				if let Some(Const::Str(bs)) = self.consts.get(ins.b as usize) {
					if bs.len() <= 5 {
						let mut bytes = [0u16; 5];
						for (i, &b) in bs.iter().enumerate() {
							bytes[i] = b as u16;
						}
						let len = bs.len() as u16;
						ins.op = Op::MkStr;
						ins.b = bytes[0] + bytes[1] * 256;
						ins.c = bytes[2] + bytes[3] * 256;
						ins.d = bytes[4] + len * 256;
					}
				}
			}
		}
		// v15 stage A: operand scattering post-pass (rewrites wire
		// operands; yields the register slot table S serialized below)
		let s_tab = if self.scatter {
			Some(self.scatter_operands(nregs))
		} else {
			None
		};
		// P1: mask MkStr packed operands with a key derived from the
		// FINAL wire register a (post-scatter): mask = (mk1*a + mk2)
		// % 65536. The handler recomputes the same key from its a
		// operand and unmasks via (x - mask) % 65536.
		let mk1 = self.program.mk1 as u32;
		let mk2 = self.program.mk2 as u32;
		for ins in self.code.iter_mut() {
			if ins.op == Op::MkStr {
				let msk = (mk1 * ins.a as u32 + mk2) % 65536;
				ins.b = ((ins.b as u32 + msk) % 65536) as u16;
				ins.c = ((ins.c as u32 + msk) % 65536) as u16;
				ins.d = ((ins.d as u32 + msk) % 65536) as u16;
			}
		}
		let map = &self.program.opmap;
		let perm = &self.program.slot_perm;
		// Nop sites are final here (no more insertions after this point)
		let nops: Vec<u16> = self
			.code
			.iter()
			.enumerate()
			.filter(|(_, i)| i.op == Op::Nop)
			.map(|(p, _)| p as u16)
			.collect();
		let mut out = Vec::with_capacity(self.code.len() * 6 + 64);
		isa::push_u16(&mut out, nregs);
		isa::push_u16(&mut out, self.nparams);
		out.push(self.vararg as u8);
		// P1: per-function constant-keystream seed (LCG state start);
		// parse mirrors the LCG (KM/KC are per-build, embedded in the
		// template) to unmask each constant payload byte.
		let ckseed = self.rng.int(0, 65535) as u16;
		isa::push_u16(&mut out, ckseed);
		isa::push_u16(&mut out, self.upsrc.len() as u16);
		for s in &self.upsrc {
			isa::push_u16(&mut out, *s);
		}
		isa::push_u16(&mut out, self.consts.len() as u16);
		// P1: the constant SECTION length prefixes the items — masked
		// type-4 varints lose self-delimiting, so the walker needs one
		// alignment anchor; parse also folds it as an integrity check.
		let mut csec: Vec<u8> = Vec::new();
		let mut lcg =
			isa::ConstLcg::new(ckseed as u32, self.program.ck_km, self.program.ck_kc);
		for c in &self.consts {
			c.encode(&mut csec, &mut lcg);
		}
		isa::push_u16(&mut out, csec.len() as u16);
		out.extend_from_slice(&csec);
		if let Some(s_tab) = &s_tab {
			isa::push_u16(&mut out, s_tab.len() as u16);
			for s in s_tab {
				isa::push_u16(&mut out, *s);
			}
		}
		// v15 stage E3: fold the sum of ALL wire operands (post-scatter,
		// mod 2^32). The v15 interpreter re-reads every operand stream via
		// inline 7-bit ladders and re-folds the same checksum (F13 shape);
		// any tamper breaks it.
		let mut osum: u64 = 0;
		for ins in &self.code {
			osum = (osum + ins.a as u64 + ins.b as u64 + ins.c as u64 + ins.d as u64) % 4294967296;
		}
		isa::encode_soa(&self.code, map, perm, &mut out);
		(out, nops, osum)
	}
}

// ---------------------------------------------------------------------------
// upvalue analysis
// ---------------------------------------------------------------------------

struct FnAnalysis {
	/// symbols referenced by this function's own code, declared outside
	direct_up: Vec<SymId>,
	/// symbols referenced by nested functions (transitively), declared
	/// outside this function -> this function must materialize them
	nested_up: Vec<SymId>,
}

/// Syms referenced by an expression at THIS level (not entering nested
/// function bodies). Globals (sym None) never enter.
fn expr_refs(e: &Expr, out: &mut HashSet<SymId>) {
	match e {
		Expr::Ident { sym: Some(s), .. } => {
			out.insert(*s);
		}
		Expr::Dot { obj, .. } => expr_refs(obj, out),
		Expr::Index { obj, idx } => {
			expr_refs(obj, out);
			expr_refs(idx, out);
		}
		Expr::Call { func, args } => {
			expr_refs(func, out);
			for a in args {
				expr_refs(a, out);
			}
		}
		Expr::Method { obj, args, .. } => {
			expr_refs(obj, out);
			for a in args {
				expr_refs(a, out);
			}
		}
		Expr::Bin { l, r, .. } => {
			expr_refs(l, out);
			expr_refs(r, out);
		}
		Expr::Un { e, .. } => expr_refs(e, out),
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => expr_refs(e, out),
					TableField::Key { key, value } => {
						expr_refs(key, out);
						expr_refs(value, out);
					}
				}
			}
		}
		_ => {}
	}
}

/// Collect all function nodes nested inside a block (any depth), plus
/// LocalFunc/FuncDecl statements, as (body, param_syms) pairs.
fn collect_bodies<'a>(block: &'a Block, out: &mut Vec<(&'a Block, &'a [SymId])>) {
	for s in &block.stmts {
		match s {
			Stmt::LocalFunc { func, .. } => out.push((&func.body, &func.param_syms)),
			Stmt::FuncDecl { func, .. } => out.push((&func.body, &func.param_syms)),
			_ => {}
		}
		match s {
			Stmt::Local { values, .. } => {
				for v in values {
					if let Some(e) = v {
						expr_bodies(e, out);
					}
				}
			}
			Stmt::Assign { values, .. } => {
				for v in values {
					expr_bodies(v, out);
				}
			}
			Stmt::ExprStmt(e) => expr_bodies(e, out),
			Stmt::Return(es) => {
				for e in es {
					expr_bodies(e, out);
				}
			}
			Stmt::If { cond, thenb, elsifs, elseb } => {
				expr_bodies(cond, out);
				collect_bodies(thenb, out);
				for (c, b) in elsifs {
					expr_bodies(c, out);
					collect_bodies(b, out);
				}
				if let Some(b) = elseb {
					collect_bodies(b, out);
				}
			}
			Stmt::While { cond, body } => {
				expr_bodies(cond, out);
				collect_bodies(body, out);
			}
			Stmt::Repeat { body, cond } => {
				collect_bodies(body, out);
				expr_bodies(cond, out);
			}
			Stmt::ForNum { start, limit, step, body, .. } => {
				expr_bodies(start, out);
				expr_bodies(limit, out);
				if let Some(st) = step {
					expr_bodies(st, out);
				}
				collect_bodies(body, out);
			}
			Stmt::ForGen { iters, body, .. } => {
				for i in iters {
					expr_bodies(i, out);
				}
				collect_bodies(body, out);
			}
			Stmt::Do(b) => collect_bodies(b, out),
			_ => {}
		}
	}
}

fn expr_bodies<'a>(e: &'a Expr, out: &mut Vec<(&'a Block, &'a [SymId])>) {
	match e {
		Expr::Function { body, param_syms, .. } => {
			out.push((body, param_syms));
			collect_bodies(body, out);
		}
		Expr::Dot { obj, .. } => expr_bodies(obj, out),
		Expr::Index { obj, idx } => {
			expr_bodies(obj, out);
			expr_bodies(idx, out);
		}
		Expr::Call { func, args } => {
			expr_bodies(func, out);
			for a in args {
				expr_bodies(a, out);
			}
		}
		Expr::Method { obj, args, .. } => {
			expr_bodies(obj, out);
			for a in args {
				expr_bodies(a, out);
			}
		}
		Expr::Bin { l, r, .. } => {
			expr_bodies(l, out);
			expr_bodies(r, out);
		}
		Expr::Un { e, .. } => expr_bodies(e, out),
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => expr_bodies(e, out),
					TableField::Key { key, value } => {
						expr_bodies(key, out);
						expr_bodies(value, out);
					}
				}
			}
		}
		_ => {}
	}
}

/// Syms declared at all depths of a block, excluding nested function
/// bodies (which have their own scopes).
fn collect_declared_all(block: &Block, out: &mut Vec<SymId>) {
	for s in &block.stmts {
		match s {
			Stmt::Local { syms, values, .. } => {
				out.extend(syms.iter().copied());
				for v in values {
					if let Some(e) = v {
						collect_declared_expr(e, out);
					}
				}
			}
			Stmt::LocalFunc { sym, .. } => out.push(*sym),
			Stmt::FuncDecl { .. } => {}
			Stmt::Assign { values, .. } => {
				for v in values {
					collect_declared_expr(v, out);
				}
			}
			Stmt::ExprStmt(e) => collect_declared_expr(e, out),
			Stmt::If { thenb, elsifs, elseb, .. } => {
				collect_declared_all(thenb, out);
				for (_, b) in elsifs {
					collect_declared_all(b, out);
				}
				if let Some(b) = elseb {
					collect_declared_all(b, out);
				}
			}
			Stmt::While { body, .. } => collect_declared_all(body, out),
			Stmt::Repeat { body, .. } => collect_declared_all(body, out),
			Stmt::Do(b) => collect_declared_all(b, out),
			Stmt::ForNum { var_sym, body, .. } => {
				out.push(*var_sym);
				collect_declared_all(body, out);
			}
			Stmt::ForGen { syms, body, .. } => {
				out.extend(syms.iter().copied());
				collect_declared_all(body, out);
			}
			_ => {}
		}
	}
}

fn collect_declared_expr(e: &Expr, out: &mut Vec<SymId>) {
	match e {
		Expr::Function { body, .. } => collect_declared_all(body, out),
		Expr::Call { func, args } => {
			collect_declared_expr(func, out);
			for a in args {
				collect_declared_expr(a, out);
			}
		}
		Expr::Method { obj, args, .. } => {
			collect_declared_expr(obj, out);
			for a in args {
				collect_declared_expr(a, out);
			}
		}
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => collect_declared_expr(e, out),
					TableField::Key { key, value } => {
						collect_declared_expr(key, out);
						collect_declared_expr(value, out);
					}
				}
			}
		}
		_ => {}
	}
}

/// All syms declared in a block subtree (loop variable of the block
/// itself included when the block is a loop body — callers add it).
fn declared_set_of(block: &Block) -> HashSet<SymId> {
	let mut v = Vec::new();
	collect_declared_all(block, &mut v);
	v.into_iter().collect()
}

/// For one loop body: the set of body-declared syms captured by ANY
/// closure created inside the body (transitively — nested functions'
/// upvalue sets included). A captured body local must live in a
/// per-iteration shared cell; an uncaptured one may reuse a fixed
/// register across iterations.
fn loop_capture_set(
	body: &Block,
	cond: Option<&Expr>,
	declared: &HashSet<SymId>,
) -> HashSet<SymId> {
	let mut bodies: Vec<(&Block, &[SymId])> = Vec::new();
	collect_bodies(body, &mut bodies);
	if let Some(c) = cond {
		expr_bodies(c, &mut bodies);
	}
	let mut cap = HashSet::new();
	for (b, p) in &bodies {
		let a = analyze(b, p);
		for &s in &a.direct_up {
			if declared.contains(&s) {
				cap.insert(s);
			}
		}
		for &s in &a.nested_up {
			if declared.contains(&s) {
				cap.insert(s);
			}
		}
	}
	cap
}

/// References at the top level of a block (excluding nested function
/// bodies). For LocalFunc/FuncDecl the body IS the function's own code —
/// its references belong to the nested function's analysis, not this
/// function's direct refs (except self-recursive references, which are
/// handled as declarations).
fn collect_block_refs_own(block: &Block, out: &mut HashSet<SymId>) {
	for s in &block.stmts {
		match s {
			Stmt::LocalFunc { .. } | Stmt::FuncDecl { .. } => {
				// FuncDecl's object expression is THIS function's code
				if let Stmt::FuncDecl { obj, .. } = s {
					if let Some(o) = obj {
						expr_refs(o, out);
					}
				}
			}
			Stmt::Local { values, .. } => {
				for v in values {
					if let Some(e) = v {
						expr_refs(e, out);
					}
				}
			}
			Stmt::Assign { targets, values } => {
			for t in targets {
				expr_refs(t, out);
			}
			for v in values {
				expr_refs(v, out);
			}
		}
		Stmt::ExprStmt(e) => expr_refs(e, out),
		Stmt::If { cond, thenb, elsifs, elseb } => {
			expr_refs(cond, out);
			refs_in_sub(thenb, out);
			for (c, b) in elsifs {
				expr_refs(c, out);
				refs_in_sub(b, out);
			}
			if let Some(b) = elseb {
				refs_in_sub(b, out);
			}
		}
		Stmt::While { cond, body } => {
			expr_refs(cond, out);
			refs_in_sub(body, out);
		}
		Stmt::Repeat { body, cond } => {
			refs_in_sub(body, out);
			expr_refs(cond, out);
		}
		Stmt::ForNum {
			start, limit, step, body, ..
		} => {
			expr_refs(start, out);
			expr_refs(limit, out);
			if let Some(st) = step {
				expr_refs(st, out);
			}
			refs_in_sub(body, out);
		}
		Stmt::ForGen { iters, body, .. } => {
			for i in iters {
				expr_refs(i, out);
			}
			refs_in_sub(body, out);
		}
		Stmt::Do(b) => refs_in_sub(b, out),
		Stmt::Return(es) => {
			for e in es {
				expr_refs(e, out);
			}
		}
		_ => {}
		}
	}
}

/// References in a sub-block, skipping the bodies of nested functions
/// (those are analyzed separately).
fn refs_in_sub(block: &Block, out: &mut HashSet<SymId>) {
	// collect_block_refs_own already skips nested function bodies
	collect_block_refs_own(block, out);
	// but it recurses into sub-blocks including ones containing nested
	// functions — the skip happens at the LocalFunc/FuncDecl level
	// (bodies not entered) ✓
}

/// Full analysis of one function body.
fn analyze(block: &Block, params: &[SymId]) -> FnAnalysis {
	let mut declared = Vec::new();
	declared.extend(params.iter().copied());
	collect_declared_all(block, &mut declared);
	let declared_set: HashSet<SymId> = declared.iter().copied().collect();

	// direct refs of this function's own code
	let mut direct = HashSet::new();
	collect_block_refs_own(block, &mut direct);
	direct.retain(|s| !declared_set.contains(s));

	// nested function refs (transitive, ANY depth): every symbol
	// referenced by a nested body that is declared neither in that
	// body's own subtree (params + all inner declarations) nor in this
	// function's subtree lives in an ANCESTOR of this function; nested
	// contexts start with a fresh scope stack, so this function must
	// upvalue+materialize it for the descriptor lookup to succeed.
	let mut bodies: Vec<(&Block, &[SymId])> = Vec::new();
	collect_bodies(block, &mut bodies);
	let mut nested = HashSet::new();
	for (b, bparams) in &bodies {
		let mut own_decl = Vec::new();
		own_decl.extend(bparams.iter().copied());
		collect_declared_all(b, &mut own_decl);
		let own_set: HashSet<SymId> = own_decl.iter().copied().collect();
		// direct refs of this nested fn's own code
		let mut refs = HashSet::new();
		collect_block_refs_own(b, &mut refs);
		refs.retain(|s| !own_set.contains(s));
		refs.retain(|s| !declared_set.contains(s));
		nested.extend(refs);
	}

	let mut direct_up: Vec<SymId> = direct.iter().copied().collect();
	direct_up.sort_unstable();
	let mut nested_up: Vec<SymId> = nested.iter().copied().collect();
	nested_up.sort_unstable();
	nested_up.dedup();
	FnAnalysis {
		direct_up,
		nested_up,
	}
}

// ---------------------------------------------------------------------------
// code generation
// ---------------------------------------------------------------------------

fn compile_chunk(
	block: &Block,
	table: &SymTable,
	rng: &mut Rng,
	lua51: bool,
	scatter: bool,
) -> VmProgram {
	let total = count_fns_count(block);
	// M5: per-build random operand slot permutation (hub randomization)
	let mut perm: Vec<u8> = vec![0, 1, 2, 3];
	rng.shuffle(&mut perm);
	let slot_perm: [u8; 4] = perm.try_into().unwrap();
	// P1 (致命缺点③): per-build constant-keystream LCG constants
	// (odd multiplier/addend, mod 2^28 — scaffold-keystream family) +
	// MkStr additive-mask constants. The template parse mirrors the
	// LCG; the MkStr handler mirrors the mask.
	let ck_km = (rng.int(100_001, 1_100_001) | 1) as u32;
	let ck_kc = (rng.int(1_000_000, 268_000_000) | 1) as u32;
	let mk1 = rng.int(1, 65535) as u16;
	let mk2 = rng.int(0, 65535) as u16;
	let mut program = VmProgram {
		opmap: OpMap::new(rng),
		slot_perm,
		carrier: Carrier::new(rng),
		// slots 0..total-1 = nested functions (DFS order); the main chunk
		// is compiled last and becomes slot `total` (PF[#FN] in the
		// template)
		fns: vec![Vec::new(); total],
		nop_sites: vec![Vec::new(); total],
		operand_sums: vec![0; total],
		ck_km,
		ck_kc,
		mk1,
		mk2,
	};
	{
		let mut ctx = Ctx::new_main(&mut program, rng, table, lua51, scatter);
		ctx.compile_block(block);
		let nregs = ctx.next_reg;
		let (bytes, nops, osum) = ctx.finish(nregs);
		program.fns.push(bytes);
		program.nop_sites.push(nops);
		program.operand_sums.push(osum);
	}
	program
}

/// Count nested function occurrences in the same DFS order the compiler
/// assigns slots.
fn count_fns(block: &Block, n: &mut usize) {
	for s in &block.stmts {
		count_stmt(s, n);
	}
}

fn count_fns_count(block: &Block) -> usize {
	let mut n = 0;
	count_fns(block, &mut n);
	n
}

fn count_stmt(s: &Stmt, n: &mut usize) {
	match s {
		Stmt::Local { values, .. } => {
			for v in values {
				if let Some(e) = v {
					count_expr(e, n);
				}
			}
		}
		Stmt::LocalFunc { func, .. } => {
			*n += 1;
			count_fns(&func.body, n);
		}
		Stmt::FuncDecl { func, obj, .. } => {
			if let Some(o) = obj {
				count_expr(o, n);
			}
			*n += 1;
			count_fns(&func.body, n);
		}
		Stmt::Assign { targets, values } => {
			for t in targets {
				count_expr(t, n);
			}
			for v in values {
				count_expr(v, n);
			}
		}
		Stmt::ExprStmt(e) => {
			count_expr(e, n);
		}
		Stmt::Return(es) => {
			for e in es {
				count_expr(e, n);
			}
		}
		Stmt::If { cond, thenb, elsifs, elseb } => {
			count_expr(cond, n);
			count_fns(thenb, n);
			for (c, b) in elsifs {
				count_expr(c, n);
				count_fns(b, n);
			}
			if let Some(b) = elseb {
				count_fns(b, n);
			}
		}
		Stmt::While { cond, body } => {
			count_expr(cond, n);
			count_fns(body, n);
		}
		Stmt::Repeat { body, cond } => {
			count_fns(body, n);
			count_expr(cond, n);
		}
		Stmt::ForNum { start, limit, step, body, .. } => {
			count_expr(start, n);
			count_expr(limit, n);
			if let Some(st) = step {
				count_expr(st, n);
			}
			count_fns(body, n);
		}
		Stmt::ForGen { iters, body, .. } => {
			for i in iters {
				count_expr(i, n);
			}
			count_fns(body, n);
		}
		Stmt::Do(b) => {
			count_fns(b, n);
		}
		_ => {}
	}
}

fn count_expr(e: &Expr, n: &mut usize) {
	match e {
		Expr::Function { body, .. } => {
			*n += 1;
			count_fns(body, n);
		}
		Expr::Dot { obj, .. } => count_expr(obj, n),
		Expr::Index { obj, idx } => {
			count_expr(obj, n);
			count_expr(idx, n);
		}
		Expr::Call { func, args } => {
			count_expr(func, n);
			for a in args {
				count_expr(a, n);
			}
		}
		Expr::Method { obj, args, .. } => {
			count_expr(obj, n);
			for a in args {
				count_expr(a, n);
			}
		}
		Expr::Bin { l, r, .. } => {
			count_expr(l, n);
			count_expr(r, n);
		}
		Expr::Un { e, .. } => count_expr(e, n),
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => count_expr(e, n),
					TableField::Key { key, value } => {
						count_expr(key, n);
						count_expr(value, n);
					}
				}
			}
		}
		_ => {}
	}
}

impl<'a> Ctx<'a> {
	fn compile_block(&mut self, block: &Block) {
		// every block is a Lua scope
		self.push_scope();
		for s in block.stmts.iter() {
			self.compile_stmt(s);
		}
		self.pop_scope();
	}

	fn compile_stmt(&mut self, s: &Stmt) {
		match s {
			Stmt::Local { names, syms, values } => {
				let _ = names;
				self.compile_local(syms, values);
			}
			Stmt::LocalFunc { name, sym, func } => {
				let _ = name;
				let r = self.tmp();
				// the function name is a local of the CURRENT scope
				// (visible to the rest of the enclosing block)
				self.declare(*sym, CellKind::Plain(r));
				self.compile_function(r, &func.body, &func.params, &func.param_syms, false);
			}
			Stmt::FuncDecl { obj, name, func, .. } => {
				if let Some(o) = obj {
					let t = self.tmp();
					self.compile_expr(o, t);
					let r = self.tmp();
					self.compile_function(
						r,
						&func.body,
						&func.params,
						&func.param_syms,
						func.has_self,
					);
					let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
					let kreg = self.tmp();
					self.emit(Instr::ab(Op::LoadK, kreg, k));
					self.emit(Instr::abc(Op::SetTab, t, kreg, r));
				} else {
					let r = self.tmp();
					self.compile_function(
						r,
						&func.body,
						&func.params,
						&func.param_syms,
						false,
					);
					let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
					self.emit(Instr::ab(Op::SetGlobal, r, k));
				}
			}
			_ => self.compile_stmt_rest(s),
		}
	}

	fn compile_local(&mut self, syms: &[SymId], values: &[Option<Expr>]) {
		let n = syms.len();
		let nv = values.len();
		let last_is_call = nv > 0
			&& values
				.last()
				.and_then(|v| v.as_ref())
				.map(is_call)
				.unwrap_or(false);
		let last_is_vararg =
			matches!(values.last(), Some(Some(e)) if matches!(e, Expr::Vararg));
		// npre = leading values assigned to targets before the final
		// (possibly multi-value) value
		let npre = if nv > 0 && (last_is_call || last_is_vararg) {
			(nv - 1).min(n)
		} else {
			nv
		};
		let mut plain: Vec<u16> = Vec::with_capacity(n);
		let mut valreg: Vec<u16> = Vec::with_capacity(n);
		for _ in 0..n {
			plain.push(self.tmp());
		}
		// leading values, source order (bounded by target count)
		let nlead = npre.min(n);
		for i in 0..nlead {
			let r = plain[i];
			match values.get(i) {
				Some(Some(e)) => {
					self.compile_expr(e, r)
				}
				_ => {
					let _ = self.emit(Instr::ab(Op::LoadNil, r, 1));
				}
			}
			valreg.push(r);
		}
		// extra values before the final one: evaluate (side effects) and
		// discard
		for i in nlead..nv.saturating_sub(1) {
			let r = self.tmp();
			match values.get(i) {
				Some(Some(e)) => {
					self.compile_expr(e, r)
				}
				_ => {
					let _ = self.emit(Instr::ab(Op::LoadNil, r, 1));
				}
			}
		}
		if nv > 0 && (last_is_call || last_is_vararg) && npre < n {
			// final value expands into targets npre..n
			let nres = n - npre;
			if last_is_call {
				let call = values[nv - 1].as_ref().unwrap();
				let (nargs, has_vararg) = call_arg_info(call);
				let freg = self.reserve(1 + nargs.max(1));
				self.compile_call_into(call, freg, nres as u16, has_vararg);
				for i in 0..nres {
					let r = plain[npre + i];
					self.emit(Instr::ab(Op::Move, r, freg + 1 + i as u16));
					valreg.push(r);
				}
			} else {
				// varargs as a table, then fetch the needed prefix
				let vt = self.tmp();
				self.emit(Instr::ab(Op::VarArgTab, vt, 0));
				for i in 0..nres {
					let kreg = self.tmp();
					let k = self.kidx(Const::Num((i + 1) as f64));
					self.emit(Instr::ab(Op::LoadK, kreg, k));
					let r = plain[npre + i];
					self.emit(Instr::abc(Op::GetTab, r, vt, kreg));
					valreg.push(r);
				}
			}
		} else if nv > 0 && npre < nv {
			// final value lies beyond the targets (extra values): evaluate
			// for side effects and discard
			let i = nv - 1;
			let r = self.tmp();
			match values.get(i) {
				Some(Some(e)) => {
					self.compile_expr(e, r)
				}
				_ => {
					let _ = self.emit(Instr::ab(Op::LoadNil, r, 1));
				}
			}
		}
		// fewer values than targets: remaining targets get nil
		while valreg.len() < n {
			let r = self.tmp();
			let knil = self.kidx(Const::Nil);
			self.emit(Instr::ab(Op::LoadK, r, knil));
			valreg.push(r);
		}
		// declare after values resolved; captured loop-body locals get a
		// per-iteration shared cell, others a plain fixed register
		for (i, s) in syms.iter().enumerate() {
			let kind = self.finalize_local(*s, plain[i], valreg[i]);
			self.declare(*s, kind);
		}
	}

	/// Create the per-iteration shared cell for a value in `valreg`:
	/// `ct = {}; ct[1] = valreg; sr = ct` and return Slot(sr). Every
	/// closure created this iteration whose descriptor points at `sr`
	/// gets the SAME cell (the interpreter binds `ups[i] = { v = V[sr],
	/// i = 1 }`), and body reads/writes go through `ct[1]` as well.
	fn cell_from_value(&mut self, valreg: u16) -> CellKind {
		let sr = self.tmp();
		let ct = self.tmp();
		let k1 = self.kidx(Const::Num(1.0));
		let kreg = self.tmp();
		self.emit(Instr::ab(Op::LoadK, kreg, k1));
		self.emit(Instr::ab(Op::NewTab, ct, 0));
		self.emit(Instr::abc(Op::SetTab, ct, kreg, valreg));
		self.emit(Instr::ab(Op::Move, sr, ct));
		CellKind::Slot(sr)
	}

	/// Store a just-evaluated local's value (in `valreg`) and return the
	/// cell kind to declare: a captured loop-body local gets a fresh
	/// per-iteration shared cell; otherwise a plain register (reused
	/// each iteration — no per-iteration growth).
	fn finalize_local(&mut self, s: SymId, plain_reg: u16, valreg: u16) -> CellKind {
		if self
			.cur_loop_captures()
			.map(|c| c.contains(&s))
			.unwrap_or(false)
		{
			return self.cell_from_value(valreg);
		}
		if valreg != plain_reg {
			self.emit(Instr::ab(Op::Move, plain_reg, valreg));
		}
		CellKind::Plain(plain_reg)
	}

	fn compile_stmt_rest(&mut self, s: &Stmt) {
		match s {
			Stmt::Assign { targets, values } => {
				// targets first (5.1 semantics), then values, then store
				let mut tinfos: Vec<Target> = Vec::new();
				for t in targets {
					tinfos.push(self.eval_target(t));
				}
				let n = targets.len();
				let nv = values.len();
				let last_is_call = nv > 0 && values.last().map(is_call).unwrap_or(false);
				let last_is_vararg =
					nv > 0 && matches!(values.last(), Some(&Expr::Vararg));
				// npre = leading values assigned to targets before the
				// final (possibly multi-value) value
				let npre = if nv > 0 && (last_is_call || last_is_vararg) {
					(nv - 1).min(n)
				} else {
					nv
				};
				// leading values, source order (bounded by target count)
				let nlead = npre.min(n);
				for i in 0..nlead {
					let r = self.tmp();
					self.compile_expr(&values[i], r);
					self.store_target(&tinfos[i], r);
				}
				// extra values before the final one: evaluate (side
				// effects) and discard
				for i in nlead..nv.saturating_sub(1) {
					let r = self.tmp();
					self.compile_expr(&values[i], r);
				}
				if nv > 0 && (last_is_call || last_is_vararg) && npre < n {
					// final value expands into targets npre..n
					let nres = (n - npre) as u16;
					if last_is_call {
						let call = values[nv - 1].clone();
						let (nargs, has_vararg) = call_arg_info(&call);
						let freg = self.reserve(1 + nargs.max(1));
						self.compile_call_into(&call, freg, nres, has_vararg);
						for i in 0..nres as usize {
							self.store_target(&tinfos[npre + i], freg + 1 + i as u16);
						}
					} else {
						// varargs as a table, then fetch the needed prefix
						let vt = self.tmp();
						self.emit(Instr::ab(Op::VarArgTab, vt, 0));
						for i in 0..nres as usize {
							let kreg = self.tmp();
							let k = self.kidx(Const::Num((i + 1) as f64));
							self.emit(Instr::ab(Op::LoadK, kreg, k));
							let r = self.tmp();
							self.emit(Instr::abc(Op::GetTab, r, vt, kreg));
							self.store_target(&tinfos[npre + i], r);
						}
					}
				} else if nv > 0 && npre < nv {
					// final value lies beyond the targets: evaluate for
					// side effects and discard
					let i = nv - 1;
					let r = self.tmp();
					self.compile_expr(&values[i], r);
				}
				// fewer values than targets: remaining targets get nil
				let knil = self.kidx(Const::Nil);
				// when the final value expands (npre < n) the leading
				// values plus the expansion cover ALL targets
				let assigned = if nv > 0 && (last_is_call || last_is_vararg) && npre < n {
					n
				} else {
					npre.min(n)
				};
				for i in assigned..n {
					let r = self.tmp();
					let _ = self.emit(Instr::ab(Op::LoadK, r, knil));
					self.store_target(&tinfos[i], r);
				}
			}
			Stmt::ExprStmt(e) => {
				let r = self.tmp();
				self.compile_expr(e, r);
			}
			Stmt::If { cond, thenb, elsifs, elseb } => {
				let t = self.tmp();
				self.compile_expr(cond, t);
				let l_end = self.new_label();
				let mut l_next = self.new_label();
				self.jmp(Op::Jf, t, &l_next);
				self.compile_block(thenb);
				self.jmp(Op::Jmp, 0, &l_end);
				for (c, b) in elsifs {
					self.here(&l_next);
					l_next = self.new_label();
					let t2 = self.tmp();
					self.compile_expr(c, t2);
					self.jmp(Op::Jf, t2, &l_next);
					self.compile_block(b);
					self.jmp(Op::Jmp, 0, &l_end);
				}
				self.here(&l_next);
				if let Some(b) = elseb {
					self.compile_block(b);
				}
				self.here(&l_end);
			}
			Stmt::While { cond, body } => {
				let cap = loop_capture_set(body, None, &declared_set_of(body));
				self.cell_loops.push(cap);
				let l_top = self.new_label();
				let l_end = self.new_label();
				self.here(&l_top);
				let t = self.tmp();
				self.compile_expr(cond, t);
				self.jmp(Op::Jf, t, &l_end);
				self.loops.push((l_end.clone(), l_top.clone()));
				self.push_scope();
				self.compile_block(body);
				self.pop_scope();
				self.loops.pop();
				self.jmp(Op::Jmp, 0, &l_top);
				self.here(&l_end);
				self.cell_loops.pop();
			}
			Stmt::Repeat { body, cond } => {
				// the until condition shares the body scope: closures in
				// it can capture body locals
				let cap = loop_capture_set(body, Some(cond), &declared_set_of(body));
				self.cell_loops.push(cap);
				let l_top = self.new_label();
				let l_check = self.new_label();
				let l_end = self.new_label();
				self.here(&l_top);
				self.loops.push((l_end.clone(), l_check.clone()));
				// 5.1 scoping: body locals are visible in the until
				// condition, so the body scope must stay open across it
				// (compile_stmt directly instead of compile_block, which
				// would pop its own scope)
				self.push_scope();
				for s in body.stmts.iter() {
					self.compile_stmt(s);
				}
				self.here(&l_check);
				let t = self.tmp();
				self.compile_expr(cond, t);
				self.jmp(Op::Jf, t, &l_top);
				self.here(&l_end);
				self.pop_scope();
				self.loops.pop();
				self.cell_loops.pop();
			}
			Stmt::ForNum {
				var,
				var_sym,
				start,
				limit,
				step,
				body,
			} => {
				let _ = var;
				let rl = self.tmp();
				let rs = self.tmp();
				let rc = self.tmp();
				self.compile_expr(start, rc);
				self.compile_expr(limit, rl);
				match step {
					Some(st) => self.compile_expr(st, rs),
					None => {
						let k = self.kidx(Const::Num(1.0));
						self.emit(Instr::ab(Op::LoadK, rs, k));
					}
				}
				let mut declared = declared_set_of(body);
				declared.insert(*var_sym);
				let cap = loop_capture_set(body, None, &declared);
				let var_captured = cap.contains(var_sym);
				self.cell_loops.push(cap);
				let l_top = self.new_label();
				let l_end = self.new_label();
				let l_inc = self.new_label();
				self.here(&l_top);
				// break if (stp >= 0 and cur > lim) or (stp < 0 and cur < lim)
				let k0 = self.kidx(Const::Num(0.0));
				let r0 = self.tmp();
				self.emit(Instr::ab(Op::LoadK, r0, k0));
				let t1 = self.tmp();
				self.emit(Instr::abc(Op::Ge, t1, rs, r0));
				let t2 = self.tmp();
				self.emit(Instr::abc(Op::Gt, t2, rc, rl));
				let t3 = self.tmp();
				self.emit_and(t3, t1, t2);
				let t4 = self.tmp();
				self.emit(Instr::abc(Op::Lt, t4, rs, r0));
				let t5 = self.tmp();
				self.emit(Instr::abc(Op::Lt, t5, rc, rl));
				let t6 = self.tmp();
				self.emit_and(t6, t4, t5);
				let t7 = self.tmp();
				self.emit_or(t7, t3, t6);
				self.jmp(Op::Jt, t7, &l_end);
				// loop variable: per-iteration SHARED cell when captured
				// by a closure (each iteration's closures — and the body
				// itself — see one cell; fresh per iteration); plain
				// fixed register otherwise (reused each iteration)
				self.loops.push((l_end.clone(), l_inc.clone()));
				self.push_scope();
				if var_captured {
					let kind = self.cell_from_value(rc);
					self.declare(*var_sym, kind);
				} else {
					let rv = self.tmp();
					self.emit(Instr::ab(Op::Move, rv, rc));
					self.declare(*var_sym, CellKind::Plain(rv));
				}
				self.compile_block(body);
				self.pop_scope();
				self.loops.pop();
				self.cell_loops.pop();
				self.here(&l_inc);
				self.emit(Instr::abc(Op::Add, rc, rc, rs));
				self.jmp(Op::Jmp, 0, &l_top);
				self.here(&l_end);
			}
			Stmt::ForGen { vars, syms, iters, body } => {
				let _ = vars;
				let mut declared = declared_set_of(body);
				for s in syms {
					declared.insert(*s);
				}
				let cap = loop_capture_set(body, None, &declared);
				let sym_captured: Vec<bool> = syms.iter().map(|s| cap.contains(s)).collect();
				self.cell_loops.push(cap);
				let rit = self.tmp();
				let rstt = self.tmp();
				let rctl = self.tmp();
			if iters.len() == 1 && is_call(&iters[0]) {
				let call = iters[0].clone();
				let (nargs, has_vararg) = call_arg_info(&call);
				// it, stt, ctl = f(...)  (nres = 3, known)
				let freg = self.reserve(1 + nargs.max(1));
				self.compile_call_into(&call, freg, 3, has_vararg);
				self.emit(Instr::ab(Op::Move, rit, freg + 1));
				self.emit(Instr::ab(Op::Move, rstt, freg + 2));
				self.emit(Instr::ab(Op::Move, rctl, freg + 3));
			} else {
				// it, stt, ctl = <iterator expression list> — a single
				// non-call expression is the iterator value itself (a
				// bare table errors at the call, mirroring 5.1; the
				// Luau `for ... in t` form is normalized to
				// `next, t` at parse time)
				if iters.len() > 0 {
					self.compile_expr(&iters[0], rit);
				}
				if iters.len() > 1 {
					self.compile_expr(&iters[1], rstt);
				}
				if iters.len() > 2 {
					self.compile_expr(&iters[2], rctl);
				}
			}
				// loop: v1..vn = it(stt, ctl); v1 == nil? break; ctl = v1; body
				let l_top = self.new_label();
				let l_end = self.new_label();
				self.here(&l_top);
				self.loops.push((l_end.clone(), l_top.clone()));
				let nv = syms.len() as u16;
				let freg = self.tmp();
				// args: stt, ctl must sit at freg+1, freg+2 at run time
				let _ = self.reserve(2 + nv.max(2).max(2));
				self.emit(Instr::ab(Op::Move, freg, rit));
				self.emit(Instr::ab(Op::Move, freg + 1, rstt));
				self.emit(Instr::ab(Op::Move, freg + 2, rctl));
				self.emit(Instr::abcd(Op::Call, freg, 2, nv, 0));
				// results at freg+1 .. freg+nv
				let tnil = self.tmp();
				let knil = self.kidx(Const::Nil);
				self.emit(Instr::ab(Op::LoadK, tnil, knil));
				let teq = self.tmp();
				self.emit(Instr::abc(Op::Eq, teq, freg + 1, tnil));
				self.jmp(Op::Jt, teq, &l_end);
				self.emit(Instr::ab(Op::Move, rctl, freg + 1));
				self.push_scope();
				for (i, s) in syms.iter().enumerate() {
					if sym_captured[i] {
						let kind = self.cell_from_value(freg + 1 + i as u16);
						self.declare(*s, kind);
					} else {
						let r = self.tmp();
						self.emit(Instr::ab(Op::Move, r, freg + 1 + i as u16));
						self.declare(*s, CellKind::Plain(r));
					}
				}
				self.compile_block(body);
				self.pop_scope();
				self.loops.pop();
				self.cell_loops.pop();
				self.jmp(Op::Jmp, 0, &l_top);
				self.here(&l_end);
			}
			Stmt::Break => {
				let (l_end, _) = *self.loops.last().expect("break outside loop");
				self.jmp(Op::Jmp, 0, &l_end);
			}
			Stmt::Continue => {
				let (_, l_cont) = *self.loops.last().expect("continue outside loop");
				self.jmp(Op::Jmp, 0, &l_cont);
			}
				Stmt::Return(es) => {
					if es.is_empty() {
						self.emit(Instr::ab(Op::Return, 0, 0));
					} else if is_call(es.last().unwrap()) {
						let pre = es.len() - 1;
						// preceding values at base+1..base+pre
						let base = self.reserve(pre as u16);
						for (i, e) in es.iter().take(pre).enumerate() {
							self.compile_expr(e, base + i as u16);
						}
						// trailing multi-value call
						let call = es.last().unwrap().clone();
						let (nargs, has_vararg) = call_arg_info(&call);
						let freg = self.reserve(1 + nargs.max(1));
						self.compile_call_into(&call, freg, 255, has_vararg);
						// RETURN: merge base+1..base+pre with lastbase+1..lastn
						self.emit(Instr::abcd(Op::Return, base, 255, 0, pre as u16));
					} else if matches!(es.last().unwrap(), Expr::Vararg) {
						// `return ..., e?` — vararg is LAST: full expansion
						let pre = es.len() - 1;
						let base = self.reserve(pre as u16);
						for (i, e) in es.iter().take(pre).enumerate() {
							self.compile_expr(e, base + i as u16);
						}
						// RETURN: merge base+1..base+pre with the varargs
						// (c = 1 selects the vararg source in the template)
						self.emit(Instr::abcd(Op::Return, base, 255, 1, pre as u16));
					} else {
						let n = es.len() as u16;
						let base = self.reserve(n);
						for (i, e) in es.iter().enumerate() {
							self.compile_expr(e, base + i as u16);
						}
						self.emit(Instr::abcd(Op::Return, base, n, 0, 0));
					}
				}
			Stmt::Do(b) => self.compile_block(b),
			_ => {}
		}
	}

	fn emit_and(&mut self, dst: u16, l: u16, r: u16) {
		self.emit(Instr::ab(Op::Move, dst, l));
		let lskip = self.new_label();
		self.jmp(Op::Jf, dst, &lskip);
		self.emit(Instr::ab(Op::Move, dst, r));
		self.here(&lskip);
	}

	fn emit_or(&mut self, dst: u16, l: u16, r: u16) {
		self.emit(Instr::ab(Op::Move, dst, l));
		let lskip = self.new_label();
		self.jmp(Op::Jt, dst, &lskip);
		self.emit(Instr::ab(Op::Move, dst, r));
		self.here(&lskip);
	}

	fn compile_expr(&mut self, e: &Expr, dst: u16) {
		match e {
			Expr::Num { value, .. } => {
				let k = self.kidx(Const::Num(*value));
				self.emit(Instr::ab(Op::LoadK, dst, k));
			}
			Expr::Str { bytes, .. } | Expr::LongStr { bytes } => {
				let k = self.kidx(Const::Str(bytes.clone()));
				self.emit(Instr::ab(Op::LoadK, dst, k));
			}
			Expr::IfExpr { arms, elseb } => {
				// per-arm cascade: test cond, fall into the value on
				// truth, jump over the rest otherwise
				let l_end = self.new_label();
				for (cond, val) in arms {
					let l_next = self.new_label();
					let t = self.tmp();
					self.compile_expr(cond, t);
					self.jmp(Op::Jf, t, &l_next);
					self.compile_expr(val, dst);
					self.jmp(Op::Jmp, 0, &l_end);
					self.here(&l_next);
				}
				self.compile_expr(elseb, dst);
				self.here(&l_end);
			}
			Expr::Bool { value } => {
				let k = self.kidx(Const::Bool(*value));
				self.emit(Instr::ab(Op::LoadK, dst, k));
			}
			Expr::Nil => {
				let k = self.kidx(Const::Nil);
				self.emit(Instr::ab(Op::LoadK, dst, k));
			}
				Expr::Vararg => {
					// value position: first vararg
					let t = self.tmp();
					self.emit(Instr::ab(Op::VarArgTab, t, 0));
					let kreg = self.tmp();
					let k1 = self.kidx(Const::Num(1.0));
					self.emit(Instr::ab(Op::LoadK, kreg, k1));
					self.emit(Instr::abc(Op::GetTab, dst, t, kreg));
				}
			Expr::Ident { name, sym } => match sym {
				Some(s) => {
					if let Some(k) = self.lookup(*s) {
						match k {
							CellKind::Plain(r) => {
								self.emit(Instr::ab(Op::Move, dst, r));
							}
							CellKind::Slot(sr) => {
								let k1 = self.kidx(Const::Num(1.0));
								let kreg = self.tmp();
								self.emit(Instr::ab(Op::LoadK, kreg, k1));
								self.emit(Instr::abc(Op::GetTab, dst, sr, kreg));
							}
							CellKind::Up(u) => {
								self.emit(Instr::ab(Op::GetUp, dst, u));
							}
						}
					} else if let Some(u) = self.upvals.get(s).copied() {
						self.emit(Instr::ab(Op::GetUp, dst, u));
					} else {
						// defensive: treat as global by name
						let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
						self.emit(Instr::ab(Op::GetGlobal, dst, k));
					}
				}
				None => {
					let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
					self.emit(Instr::ab(Op::GetGlobal, dst, k));
				}
			},
			Expr::Dot { obj, name } => {
				let t = self.tmp();
				self.compile_expr(obj, t);
				let kreg = self.tmp();
				let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
				self.emit(Instr::ab(Op::LoadK, kreg, k));
				self.emit(Instr::abc(Op::GetTab, dst, t, kreg));
			}
			Expr::Index { obj, idx } => {
				let t = self.tmp();
				let k = self.tmp();
				self.compile_expr(obj, t);
				self.compile_expr(idx, k);
				self.emit(Instr::abc(Op::GetTab, dst, t, k));
			}
			Expr::Bin { op, l, r } => match op {
				BinOp::And => {
					self.compile_expr(l, dst);
					let lskip = self.new_label();
					self.jmp(Op::Jf, dst, &lskip);
					self.compile_expr(r, dst);
					self.here(&lskip);
				}
				BinOp::Or => {
					self.compile_expr(l, dst);
					let lskip = self.new_label();
					self.jmp(Op::Jt, dst, &lskip);
					self.compile_expr(r, dst);
					self.here(&lskip);
				}
				_ => {
					let tl = self.tmp();
					let tr = self.tmp();
					self.compile_expr(l, tl);
					self.compile_expr(r, tr);
					let iop = match op {
						BinOp::Add => Op::Add,
						BinOp::Sub => Op::Sub,
						BinOp::Mul => Op::Mul,
						BinOp::Div => Op::Div,
						BinOp::Idiv => Op::Idiv,
						BinOp::Mod => Op::Mod,
						BinOp::Pow => Op::Pow,
						BinOp::Concat => Op::Concat,
						BinOp::Eq => Op::Eq,
						BinOp::Ne => Op::Ne,
						BinOp::Lt => Op::Lt,
						BinOp::Gt => Op::Gt,
						BinOp::Le => Op::Le,
						BinOp::Ge => Op::Ge,
						_ => unreachable!(),
					};
					self.emit(Instr::abc(iop, dst, tl, tr));
				}
			},
			Expr::Un { op, e } => {
				if *op == UnOp::Len && matches!(e.as_ref(), Expr::Vararg) {
					self.emit(Instr::ab(Op::VarArgC, dst, 0));
					return;
				}
				let t = self.tmp();
				self.compile_expr(e, t);
				let uop = match op {
					UnOp::Minus => Op::Unm,
					UnOp::Not => Op::Not,
					UnOp::Len => Op::Len,
				};
				self.emit(Instr::ab(uop, dst, t));
			}
			Expr::Call { .. } => {
				let (nargs, has_vararg) = call_arg_info(e);
				let freg = self.reserve(1 + nargs.max(1));
				self.compile_call_into(e, freg, 1, has_vararg);
				if freg + 1 != dst {
					self.emit(Instr::ab(Op::Move, dst, freg + 1));
				}
			}
			Expr::Method { obj, name, args } => {
				let t = self.tmp();
				self.compile_expr(obj, t);
				let kreg = self.tmp();
				let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
				self.emit(Instr::ab(Op::LoadK, kreg, k));
				let n = (args.len() + 1) as u16;
				let freg = self.reserve(1 + n);
				// freg = method, freg+1 = obj, freg+2.. = args
				self.emit(Instr::abc(Op::GetTab, freg, t, kreg));
				self.emit(Instr::ab(Op::Move, freg + 1, t));
				for (i, a) in args.iter().enumerate() {
					self.compile_expr(a, freg + 2 + i as u16);
				}
				self.emit(Instr::abcd(Op::Call, freg, n, 1, 0));
				if freg + 1 != dst {
					self.emit(Instr::ab(Op::Move, dst, freg + 1));
				}
			}
			Expr::Table { fields } => {
				self.emit(Instr::ab(Op::NewTab, dst, 0));
				let cnt = self.tmp();
				let k0 = self.kidx(Const::Num(0.0));
				self.emit(Instr::ab(Op::LoadK, cnt, k0));
				let last = fields.len().saturating_sub(1);
				if self.lua51 {
					// 5.1 store order (verified against luac -l): field
					// VALUES evaluate in source order, Key stores
					// (SETTABLE) execute in source order, and the array
					// part (SETLIST) is flushed LAST — so positional
					// fields win over duplicate [i] keys regardless of
					// source order.
					let mut pending: Vec<u16> = Vec::new();
					let mut tail_call: Option<Expr> = None;
					let mut tail_vararg = false;
					for (i, f) in fields.iter().enumerate() {
						match f {
							TableField::Key { key, value } => {
								let kv = self.tmp();
								let vv = self.tmp();
								self.compile_expr(key, kv);
								self.compile_expr(value, vv);
								self.emit(Instr::abc(Op::SetTab, dst, kv, vv));
							}
							TableField::Array(e) => {
								if i == last && is_call(e) {
									// trailing call: its expanded store is
									// necessarily last — defer emission
									tail_call = Some(e.clone());
								} else if i == last && matches!(e, Expr::Vararg) {
									tail_vararg = true;
								} else {
									// truncated to the first value
									let v = self.tmp();
									self.compile_expr(e, v);
									pending.push(v);
								}
							}
						}
					}
					for v in pending {
						self.emit(Instr::abc(Op::TabN, dst, cnt, v));
					}
					if tail_vararg {
						self.emit(Instr::ab(Op::VarArgTabN, dst, cnt));
					}
					if let Some(e) = tail_call {
						let (nargs, has_vararg) = call_arg_info(&e);
						let _ = has_vararg; // table context: no vararg
						let freg = self.reserve(1 + nargs.max(1));
						self.compile_call_t(&e, freg, dst, cnt);
					}
				} else {
					// Luau: stores in source order (last write wins for
					// duplicate keys)
					for (i, f) in fields.iter().enumerate() {
						match f {
							TableField::Array(e) => {
								if i == last && is_call(e) {
									// trailing call: CALLT (multi-value
									// into the table, counter advances)
									let (nargs, has_vararg) = call_arg_info(e);
									let _ = has_vararg; // no vararg here
									let freg = self.reserve(1 + nargs.max(1));
									self.compile_call_t(e, freg, dst, cnt);
								} else if i == last && matches!(e, Expr::Vararg) {
									self.emit(Instr::ab(Op::VarArgTabN, dst, cnt));
								} else {
									let v = self.tmp();
									self.compile_expr(e, v);
									self.emit(Instr::abc(Op::TabN, dst, cnt, v));
								}
							}
							TableField::Key { key, value } => {
								let kv = self.tmp();
								let vv = self.tmp();
								self.compile_expr(key, kv);
								self.compile_expr(value, vv);
								self.emit(Instr::abc(Op::SetTab, dst, kv, vv));
							}
						}
					}
				}
			}
			Expr::Function {
				params,
				param_syms,
				vararg,
				body,
			} => {
				self.compile_function(dst, body, params, param_syms, *vararg);
			}
		}
	}

	/// The trailing argument of a call expression, if any.
	fn call_tail<'e>(e: &'e Expr) -> Option<&'e Expr> {
		let args = match e {
			Expr::Call { args, .. } => args,
			Expr::Method { args, .. } => args,
			_ => return None,
		};
		args.last().filter(|a| is_call(a))
	}

	/// (fixed arg count, has trailing call) for `e`; the method count
	/// includes the self slot.
	fn call_nfixed(e: &Expr) -> (u16, bool) {
		let (args, is_m) = match e {
			Expr::Call { args, .. } => (args, false),
			Expr::Method { args, .. } => (args, true),
			_ => unreachable!(),
		};
		let has_tail = args.last().map(is_call).unwrap_or(false);
		let fixed = if has_tail {
			(args.len() - 1) as u16 + if is_m { 1 } else { 0 }
		} else {
			args.iter().filter(|a| !matches!(a, Expr::Vararg)).count() as u16
				+ if is_m { 1 } else { 0 }
		};
		(fixed, has_tail)
	}

	/// Compile the callee plus the first `nfixed` arg slots (each a
	/// single value): callee at freg, args at freg+1.. (freg+2.. for
	/// methods). The tail call (if any) is NOT compiled here.
	fn compile_call_prefix(&mut self, e: &Expr, freg: u16, nfixed: u16) {
		match e {
			Expr::Call { func, args } => {
				self.compile_expr(func, freg);
				for (i, a) in args.iter().enumerate() {
					if (i as u16) >= nfixed {
						break;
					}
					if matches!(a, Expr::Vararg) {
						continue;
					}
					self.compile_expr(a, freg + 1 + i as u16);
				}
			}
			Expr::Method { obj, name, args } => {
				let treg = self.tmp();
				self.compile_expr(obj, treg);
				let kreg = self.tmp();
				let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
				self.emit(Instr::ab(Op::LoadK, kreg, k));
				self.emit(Instr::abc(Op::GetTab, freg, treg, kreg));
				self.emit(Instr::ab(Op::Move, freg + 1, treg));
				// nfixed includes the self slot: compile nfixed-1 args
				for (i, a) in args.iter().enumerate() {
					if (i as u16) >= nfixed - 1 {
						break;
					}
					if matches!(a, Expr::Vararg) {
						continue;
					}
					self.compile_expr(a, freg + 2 + i as u16);
				}
			}
			_ => unreachable!(),
		}
	}

	/// Compile `e` as a call whose results are FULLY EXPANDED: results
	/// overwrite the args at freg+1.. and the result count is stored in
	/// the function slot freg (consumed by a CallE/CallM above). A
	/// trailing call in `e` is itself expanded (recursive CallE chain).
	fn compile_call_expand(&mut self, e: &Expr, freg: u16) {
		let (nfixed, has_tail) = Self::call_nfixed(e);
		self.compile_call_prefix(e, freg, nfixed);
		let mut d: u16 = 0;
		if has_tail {
			let tail = Self::call_tail(e).unwrap().clone();
			let (inargs, _) = call_arg_info(&tail);
			let ifreg = freg + nfixed + 1;
			if inargs > 0 {
				self.reserve(inargs);
			}
			self.compile_call_expand(&tail, ifreg);
			d |= 2;
		}
		let varg = match e {
			Expr::Call { args, .. } => args.iter().any(|a| matches!(a, Expr::Vararg)),
			Expr::Method { args, .. } => args.iter().any(|a| matches!(a, Expr::Vararg)),
			_ => false,
		};
		if varg {
			d |= 1;
		}
		self.emit(Instr::abcd(Op::CallE, freg, nfixed, 0, d));
	}

	/// Compile `call` with the function register at `freg` (args at
	/// freg+1..freg+nargs), `nres` results (255 = variable), optional
	/// vararg append. Results land at freg+1.. (over the args). A
	/// trailing call arg expands all its results (5.1 call-arg rule).
	fn compile_call_into(&mut self, e: &Expr, freg: u16, nres: u16, has_vararg: bool) {
		let (nfixed, has_tail) = Self::call_nfixed(e);
		self.compile_call_prefix(e, freg, nfixed);
		if has_tail {
			let tail = Self::call_tail(e).unwrap().clone();
			let (inargs, _) = call_arg_info(&tail);
			let ifreg = freg + nfixed + 1;
			if inargs > 0 {
				self.reserve(inargs);
			}
			self.compile_call_expand(&tail, ifreg);
			self.emit(Instr::abcd(Op::CallM, freg, nfixed, nres, has_vararg as u16));
		} else {
			self.emit(Instr::abcd(Op::Call, freg, nfixed, nres, has_vararg as u16));
		}
	}

	/// Trailing table call: CALLT — f at freg, args freg+1.., results into
	/// `t` from counter+1, counter advances. A trailing call arg expands
	/// (d = nfixed*2 + tail_flag).
	fn compile_call_t(&mut self, e: &Expr, freg: u16, t: u16, cnt: u16) {
		let (nfixed, has_tail) = Self::call_nfixed(e);
		self.compile_call_prefix(e, freg, nfixed);
		let mut d = nfixed * 2;
		if has_tail {
			let tail = Self::call_tail(e).unwrap().clone();
			let (inargs, _) = call_arg_info(&tail);
			let ifreg = freg + nfixed + 1;
			if inargs > 0 {
				self.reserve(inargs);
			}
			self.compile_call_expand(&tail, ifreg);
			d |= 1;
		}
		self.emit(Instr::abcd(Op::CallT, freg, t, cnt, d));
	}

	fn compile_function(
		&mut self,
		dst: u16,
		body: &Block,
		params: &[String],
		param_syms: &[SymId],
		vararg: bool,
	) {
		let _ = params;
		let analysis = analyze(body, param_syms);

		// upvalue list = materialize (nested refs) ∪ direct refs
		let mut ups: Vec<SymId> = analysis.nested_up.clone();
		for s in &analysis.direct_up {
			if !ups.contains(s) {
				ups.push(*s);
			}
		}
		ups.sort_unstable();
		ups.dedup();
		if std::env::var("LURAPH_VM_DBG").is_ok() {
			let nm = |v: Vec<SymId>| {
				let t: Vec<String> = v
					.iter()
					.map(|&s| format!("{}({})", self.symtab.name_of(s), s))
					.collect();
				t.join(",")
			};
			let scopes_dbg: Vec<String> = self
				.scopes
				.iter()
				.map(|sc| {
					let e: Vec<String> = sc
						.iter()
						.map(|(s, r)| format!("{}({})@{:?}", self.symtab.name_of(*s), s, r))
						.collect();
					e.join("|")
				})
				.collect();
			eprintln!(
				"[fn] direct_up=[{}] nested_up=[{}] ups=[{}]",
				nm(analysis.direct_up.clone()),
				nm(analysis.nested_up.clone()),
				nm(ups.clone())
			);
			eprintln!("[fn] scopes={:?}", scopes_dbg);
		}

		// descriptors = the innermost enclosing scope holding the sym:
		// Plain -> slot number; Slot (per-iteration cell) -> 0x8000 +
		// slot — the interpreter binds it to the CURRENT iteration's
		// cell table at closure creation; Up (materialized upvalue) ->
		// 0xC000 + upvalue index — the interpreter aliases the CREATING
		// frame's own cell object (one canonical cell at all levels)
		let mut descriptors: Vec<u16> = Vec::new();
		for s in &ups {
			let mut found = None;
			for sc in self.scopes.iter().rev() {
				if let Some(&k) = sc.get(s) {
					found = Some(k);
					break;
				}
			}
			match found {
				Some(CellKind::Plain(r)) => descriptors.push(r + 1),
				Some(CellKind::Slot(sr)) => descriptors.push(sr + 1 | 0x8000),
				Some(CellKind::Up(u)) => descriptors.push(u + 1 | 0xC000),
				None => panic!(
					"upvalue descriptor miss: sym {} not in any enclosing scope (ups mis-analyzed?)",
					s
				),
			}
		}

		let child_index = self.next_fn_slot;
		self.next_fn_slot += 1;
		let nparams = param_syms.len() as u16;
		let mut child = Ctx {
			program: self.program,
			rng: self.rng,
			symtab: self.symtab,
			lua51: self.lua51,
			scatter: self.scatter,
			next_fn_slot: self.next_fn_slot,
			scopes: vec![HashMap::new()],
			upvals: HashMap::new(),
			upsrc: descriptors,
			// temporaries start AFTER the parameter registers
			next_reg: nparams,
			nparams,
			vararg,
			consts: Vec::new(),
			const_map: HashMap::new(),
			code: Vec::new(),
			labels: HashMap::new(),
			loops: Vec::new(),
			cell_loops: Vec::new(),
		};
		for (i, s) in ups.iter().enumerate() {
			child.upvals.insert(*s, i as u16);
		}
		// params occupy regs 0..nparams-1
		child.push_scope();
		for (i, s) in param_syms.iter().enumerate() {
			child.declare(*s, CellKind::Plain(i as u16));
		}
		// materialize ALL upvalues (direct + nested) by ALIAS: the child
		// scope maps the symbol straight to its upvalue cell (no value
		// copy — 5.1 keeps ONE cell per local for every closure, and an
		// intermediate copy would desync reads/writes across nesting
		// levels). GRANDCHILD descriptor lookups still find the symbol
		// in this function's scope (a grandchild may upvalue a symbol
		// this function only DIRECTLY references — it must be
		// materialized here or the grandchild's descriptor lookup misses
		// it), and its descriptor becomes an upvalue-alias.
		for s in &ups {
			let u = child.upvals[s];
			child.declare(*s, CellKind::Up(u));
		}
		child.compile_block(body);
		child.pop_scope();
		let slot_end = child.next_fn_slot;
		let nregs = child.next_reg;
		let (bytes, nops, osum) = child.finish(nregs);
		self.next_fn_slot = slot_end;
		if child_index >= self.program.fns.len() { panic!("slot overflow: child_index={} len={} next={}", child_index, self.program.fns.len(), self.next_fn_slot); }
		self.program.fns[child_index] = bytes;
		self.program.nop_sites[child_index] = nops;
		self.program.operand_sums[child_index] = osum;

		self.emit(Instr::ab(Op::Closure, dst, child_index as u16));
	}

	fn eval_target(&mut self, t: &Expr) -> Target {
		match t {
			Expr::Ident { name, sym } => match sym {
				Some(s) => Target::Local(*s),
				None => Target::Global(self.kidx(Const::Str(name.as_bytes().to_vec()))),
			},
			Expr::Dot { obj, name } => {
				let treg = self.tmp();
				self.compile_expr(obj, treg);
				let kreg = self.tmp();
				let k = self.kidx(Const::Str(name.as_bytes().to_vec()));
				// the key must live in a register (SetTab reads V[k+1])
				self.emit(Instr::ab(Op::LoadK, kreg, k));
				Target::Index2 { t: treg, k: kreg }
			}
			Expr::Index { obj, idx } => {
				let treg = self.tmp();
				let kreg = self.tmp();
				self.compile_expr(obj, treg);
				self.compile_expr(idx, kreg);
				Target::Index2 { t: treg, k: kreg }
			}
			_ => panic!("invalid assignment target"),
		}
	}

	fn store_target(&mut self, t: &Target, vreg: u16) {
		match t {
			Target::Local(s) => {
				if let Some(k) = self.lookup(*s) {
					match k {
						CellKind::Plain(r) => {
							self.emit(Instr::ab(Op::Move, r, vreg));
							// a materialized upvalue is a LOCAL register
							// here, but the value lives in a shared cell:
							// forward the write so sibling closures and the
							// creating frame observe it
							if let Some(u) = self.upvals.get(&s).copied() {
								self.emit(Instr::ab(Op::SetUp, u, r));
							}
						}
						CellKind::Slot(sr) => {
							let k1 = self.kidx(Const::Num(1.0));
							let kreg = self.tmp();
							self.emit(Instr::ab(Op::LoadK, kreg, k1));
							self.emit(Instr::abc(Op::SetTab, sr, kreg, vreg));
						}
						CellKind::Up(u) => {
							// write straight through the canonical cell
							self.emit(Instr::ab(Op::SetUp, u, vreg));
						}
					}
				} else if let Some(u) = self.upvals.get(s).copied() {
					self.emit(Instr::ab(Op::SetUp, u, vreg));
				} else {
					panic!("store to unknown local");
				}
			}
			Target::Global(k) => {
				self.emit(Instr::ab(Op::SetGlobal, vreg, *k));
			}
			Target::Index2 { t, k } => {
				self.emit(Instr::abc(Op::SetTab, *t, *k, vreg));
			}
		}
	}
}

#[derive(Debug, Clone)]
enum Target {
	Local(SymId),
	Global(u16),
	/// table register + key register (key is materialized by the time
	/// eval_target returns, so it survives value compilation)
	Index2 { t: u16, k: u16 },
}

fn is_call(e: &Expr) -> bool {
	matches!(e, Expr::Call { .. } | Expr::Method { .. })
}

fn call_arg_info(e: &Expr) -> (u16, bool) {
	match e {
		Expr::Call { args, .. } => {
			let n = args
				.iter()
				.filter(|a| !matches!(a, Expr::Vararg))
				.count() as u16;
			let v = args.iter().any(|a| matches!(a, Expr::Vararg));
			(n, v)
		}
		Expr::Method { args, .. } => ((args.len() + 1) as u16, false),
		_ => (0, false),
	}
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

pub fn compile(
	block: &Block,
	table: &SymTable,
	rng: &mut Rng,
	lua51: bool,
	scatter: bool,
) -> VmProgram {
	compile_chunk(block, table, rng, lua51, scatter)
}

#[cfg(test)]
mod dbg {
	use super::*;
	use crate::parser;
	use crate::symtab;

	#[test]
	#[ignore] // dev tool: needs /tmp/nt5.lua
	fn dbg_header() {
		let src = std::fs::read_to_string("/tmp/nt5.lua").unwrap();
		let mut block = parser::parse(&src, false).unwrap();
		let mut table = symtab::resolve(&mut block);
		eprintln!("count_fns={}", count_fns_count(&block));
		if let crate::ast::Stmt::If { thenb, .. } = &block.stmts[0] {
			eprintln!("thenb stmts: {}", thenb.stmts.len());
			for (si, s) in thenb.stmts.iter().enumerate() {
				eprintln!("  thenb[{}] = {:?}", si, std::mem::discriminant(s));
				if let crate::ast::Stmt::Local { values, .. } = s {
					for (vi, v) in values.iter().enumerate() {
						eprintln!("    value[{}] = {:?}", vi, v.as_ref().map(|e| std::mem::discriminant(e)));
					}
				}
			}
		}
		for (si, s) in block.stmts.iter().enumerate() {
			eprintln!("stmt[{}] = {:?}", si, std::mem::discriminant(s));
			if let crate::ast::Stmt::Local { values, .. } = s {
				for (vi, v) in values.iter().enumerate() {
					if let Some(e) = v {
						eprintln!("  value[{}] = {:?}", vi, std::mem::discriminant(e));
						if let crate::ast::Expr::Function { body, .. } = e {
							eprintln!("    fn body stmts: {}", body.stmts.len());
						}
					} else {
						eprintln!("  value[{}] = None", vi);
					}
				}
			}
		}
		let mut rng = crate::rng::Rng::new(42);
		let prog = compile_chunk(&block, &table, &mut rng, true, false);
		eprintln!("fns={}", prog.fns.len());
		let names = ["Jmp","Jf","Jt","LoadNil","LoadK","Move","Add","Sub","Mul","Div","Mod","Pow","Concat","Unm","Not","Len","Lt","Le","Gt","Ge","Eq","Ne","Idiv","NewTab","GetTab","SetTab","TabN","CallT","Closure","Call","VarArgTab","VarArgC","VarArgTabN","GetGlobal","SetGlobal","GetUp","SetUp","Return","Nop","CallE","CallM"];
		let mut wire2name: Vec<Option<&str>> = vec![None; 256];
		for (i, nm) in names.iter().enumerate() {
			wire2name[prog.opmap.to_wire[i] as usize] = Some(*nm);
		}
		for (fi, b) in prog.fns.iter().enumerate() {
			eprintln!("=== FN[{}] len={}", fi, b.len());
			let u16 = |p: usize| (b[p] as u16) | ((b[p + 1] as u16) << 8);
			let mut p = 0;
			let nregs = u16(p); p += 2;
			let nparams = u16(p); p += 2;
			let vararg = b[p]; p += 1;
			let nups = u16(p); p += 2;
			eprintln!("nregs={nregs} nparams={nparams} vararg={vararg} nups={nups}");
			p += 2 * nups as usize;
			let nconst = u16(p); p += 2;
			for i in 0..nconst as usize {
				let t = b[p]; p += 1;
				match t {
					0 => eprintln!("  C[{i}] = nil"),
					1 => { eprintln!("  C[{i}] = bool {}", b[p] == 1); p += 1; }
					_ => {
						let l = u16(p) as usize; p += 2;
						let txt = String::from_utf8_lossy(&b[p..p + l]).to_string();
						let kind = if t == 2 { "num" } else { "str" };
						eprintln!("  C[{i}] = {kind} {:?}", &txt[..txt.len().min(20)]);
						p += l;
					}
				}
			}
			// SoA: [ncode u16][OC bytes][4 × ncode varints]
			let perm = &prog.slot_perm;
			let ncode = u16(p) as usize;
			p += 2;
			let mut ops: Vec<u8> = Vec::with_capacity(ncode);
			for _ in 0..ncode {
				ops.push(b[p]);
				p += 1;
			}
			let mut streams = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
			for s in 0..4 {
				for _ in 0..ncode {
					let (v, np) = isa::decode_varint(b, p).expect("varint");
					streams[s].push(v as u16);
					p = np;
				}
			}
			for i in 0..ncode {
				let nm = wire2name[ops[i] as usize].unwrap_or("??");
				let mut vals = [0u16; 4];
				for s in 0..4 {
					vals[perm[s] as usize] = streams[s][i];
				}
				eprintln!(
					"  [{i}] {nm} a={} b={} c={} d={}",
					vals[0], vals[1], vals[2], vals[3]
				);
			}
		}
	}

	/// v15 stage A: verify the scatter post-pass — S slots distinct and
	/// spread beyond the dense range, wire operands scattered, consts
	/// padded.
	#[test]
	fn scatter_layout_properties() {
		let src = "local function f(a, b, c) local x = a + b * c; local y = x - a; return y, x end return f(1, 2, 3)";
		let mut block = parser::parse(src, true).unwrap();
		let table = symtab::resolve(&mut block);
		let mut rng = crate::rng::Rng::new(7);
		let prog = compile_chunk(&block, &table, &mut rng, false, true);
		let mut saw_scattered_operand = false;
		for b in &prog.fns {
			let u16 = |p: usize| (b[p] as u16) | ((b[p + 1] as u16) << 8);
			let mut p = 0;
			let nregs = u16(p) as usize; p += 2;
			p += 2; // nparams
			p += 1; // vararg
			p += 2; // P1: ckseed (常量密钥流种子)
			let nups = u16(p) as usize; p += 2 + 2 * nups;
			let _nconst = u16(p) as usize; p += 2;
			// P1: constant section length anchor (masked type-4 items
			// are not self-delimiting)
			let seclen = u16(p) as usize; p += 2 + seclen;
			let ns = u16(p) as usize; p += 2;
			assert_eq!(ns, nregs, "S table length must equal nregs");
			let mut slots: Vec<u16> = Vec::new();
			for _ in 0..ns { slots.push(u16(p)); p += 2; }
			let mut sorted = slots.clone();
			sorted.sort_unstable(); sorted.dedup();
			assert_eq!(sorted.len(), ns, "S slots must be distinct");
			let smax = nregs + (nregs / 2).min(64).max(8);
			for &s in &slots {
				assert!((1..=smax as u16).contains(&s), "slot in range");
			}
			if nregs >= 4 {
				assert!(
					*slots.iter().max().unwrap() as usize > nregs,
					"slots must be spread beyond the dense range"
				);
			}
			// wire operands: decode streams; single-register operands
			// of arithmetic ops must reach beyond nregs somewhere
			let ncode = u16(p) as usize; p += 2;
			p += ncode; // skip the opcode array
			let perm = &prog.slot_perm;
			let mut streams = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
			for s in 0..4 {
				for _ in 0..ncode {
					let (v, np) = isa::decode_varint(b, p).expect("varint");
					streams[s].push(v as u16);
					p = np;
				}
			}
			for i in 0..ncode {
				let mut vals = [0u16; 4];
				for s in 0..4 {
					vals[perm[s] as usize] = streams[s][i];
				}
				if vals.iter().any(|&v| v as usize > nregs) {
					saw_scattered_operand = true;
				}
			}
		}
		assert!(saw_scattered_operand, "some wire operand must be scattered");
	}
}
