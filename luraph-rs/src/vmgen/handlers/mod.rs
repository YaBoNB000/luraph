//! L6 VM — per-instruction interpreter handlers (建议1).
//!
//! Every VM opcode lives in its OWN file (jmp.rs, add.rs, ...) and
//! returns its fixed interpreter code; the assembler asks each file for
//! the opcodes it dispatches ("用哪个就返回哪个").
//!
//! 进阶 (建议1 advanced): each instruction carries several semantically
//! identical FORMATS; every build picks one format per instruction at
//! random, the dispatch leaf order is shuffled per generation and the
//! wire codes are a per-build permutation (OpMap in isa.rs) — the VM
//! layout is different on every build.
//!
//! Format variants are pure surface transformations (local renames,
//! type-check hoisting, equivalent loop forms): semantics are identical
//! by construction, and the full corpus matrix (lua51 + luau, VM on/off,
//! multi-seed) gates every change.

use crate::vmgen::strpool::StrPool;

pub mod add;
pub mod call;
pub mod calle;
pub mod callm;
pub mod callt;
pub mod closure;
pub mod concat;
pub mod div;
pub mod eq;
pub mod ge;
pub mod getglobal;
pub mod gettab;
pub mod getup;
pub mod gt;
pub mod idiv;
pub mod jf;
pub mod jmp;
pub mod jt;
pub mod le;
pub mod len;
pub mod loadk;
pub mod loadnil;
pub mod lt;
pub mod mkstr;
pub mod mod_;
#[path = "move.rs"]
pub mod move_;
pub mod mul;
pub mod ne;
pub mod newtab;
pub mod nop;
pub mod not_;
pub mod pow;
pub mod ret;
pub mod setglobal;
pub mod settab;
pub mod setup;
pub mod sub;
pub mod tabn;
pub mod unm;
pub mod vargc;
pub mod vargtab;
pub mod vargtabn;

/// How many formats an instruction carries in the given profile.
pub fn n_formats(name: &str, v15: bool) -> u8 {
	match name {
		// v15 Return must keep the exact `return U(out, 1, total)`
		// tail so the CPS rewrite (signal-table form) can match it.
		"Return" if v15 => 1,
		_ => match name {
			"Add" | "Sub" | "Mul" | "Div" | "Mod" | "Pow" => 3,
			"Lt" | "Le" | "Gt" | "Ge" => 3,
			_ => 2,
		},
	}
}

/// The interpreter body of one opcode, in the format chosen for this
/// build. `v15` selects the operand-scattering variant for the opcodes
/// that walk contiguous register ranges (isa stage A). `mk` = the
/// per-build MkStr mask constants (used only by mkstr.rs).
pub fn gen(name: &str, fmt: u8, v15: bool, p: &mut StrPool, mk: (u16, u16)) -> String {
	match name {
		"Jmp" => jmp::code(fmt),
		"Jf" => jf::code(fmt),
		"Jt" => jt::code(fmt),
		"LoadNil" => {
			if v15 {
				loadnil::code_v15(fmt)
			} else {
				loadnil::code(fmt)
			}
		}
		"LoadK" => loadk::code(fmt),
		"Move" => move_::code(fmt),
		"Add" => add::code(fmt, p),
		"Sub" => sub::code(fmt, p),
		"Mul" => mul::code(fmt, p),
		"Div" => div::code(fmt, p),
		"Mod" => mod_::code(fmt, p),
		"Pow" => pow::code(fmt, p),
		"Concat" => concat::code(fmt, p),
		"Unm" => unm::code(fmt, p),
		"Not" => not_::code(fmt),
		"Len" => len::code(fmt, p),
		"Lt" => lt::code(fmt, p),
		"Le" => le::code(fmt, p),
		"Gt" => gt::code(fmt, p),
		"Ge" => ge::code(fmt, p),
		"Eq" => eq::code(fmt, p),
		"Ne" => ne::code(fmt, p),
		"Idiv" => idiv::code(fmt),
		"NewTab" => newtab::code(fmt),
		"GetTab" => gettab::code(fmt, p),
		"SetTab" => settab::code(fmt, p),
		"TabN" => tabn::code(fmt),
		"CallT" => {
			if v15 {
				callt::code_v15(fmt)
			} else {
				callt::code(fmt)
			}
		}
		"Closure" => {
			if v15 {
				closure::code_v15(fmt)
			} else {
				closure::code(fmt)
			}
		}
		"Call" => {
			if v15 {
				call::code_v15(fmt)
			} else {
				call::code(fmt)
			}
		}
		"VarArgTab" => vargtab::code(fmt),
		"VarArgC" => vargc::code(fmt),
		"VarArgTabN" => vargtabn::code(fmt),
		"GetGlobal" => getglobal::code(fmt),
		"SetGlobal" => setglobal::code(fmt),
		"GetUp" => getup::code(fmt),
		"SetUp" => setup::code(fmt),
		"Return" => {
			if v15 {
				ret::code_v15()
			} else {
				ret::code(fmt)
			}
		}
		"CallE" => {
			if v15 {
				calle::code_v15(fmt)
			} else {
				calle::code(fmt)
			}
		}
		"CallM" => {
			if v15 {
				callm::code_v15(fmt)
			} else {
				callm::code(fmt)
			}
		}
		"MkStr" => mkstr::code(fmt, mk),
		_ => panic!("unknown opcode name {name}"),
	}
}

/// Shared format builder: the six arithmetic two-operand handlers
/// (Add/Sub/Mul/Div/Mod/Pow) differ only in metamethod name and the
/// raw expression. Formats:
///   f0 — canonical shape (x/y temps, inline TYP checks)
///   f1 — renamed temporaries (u/w)
///   f2 — TYP results hoisted into locals before the branch
pub(crate) fn bin_op_body(
	fmt: u8,
	mm: &str,
	expr: &dyn Fn(&str, &str) -> String,
	p: &mut StrPool,
) -> String {
	let number = p.lit("number");
	let meta = p.lit(mm);
	let msg = p.lit("attempt to perform arithmetic on a ");
	let value = p.lit(" value");
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local y = V[c + 1]; if TYP(x) == {number} and TYP(y) == {number} then V[a + 1] = {exy} else local f = mget(x, {meta}) or mget(y, {meta}); if f then V[a + 1] = f(x, y) else ERR({msg} .. TYP(x) .. {value}, 0) end end",
			exy = expr("x", "y")
		),
		1 => format!(
			"local u = V[b + 1]; local w = V[c + 1]; if TYP(u) == {number} and TYP(w) == {number} then V[a + 1] = {euw} else local f = mget(u, {meta}) or mget(w, {meta}); if f then V[a + 1] = f(u, w) else ERR({msg} .. TYP(u) .. {value}, 0) end end",
			euw = expr("u", "w")
		),
		_ => format!(
			"local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if tx == {number} and ty == {number} then V[a + 1] = {exy} else local f = mget(x, {meta}) or mget(y, {meta}); if f then V[a + 1] = f(x, y) else ERR({msg} .. tx .. {value}, 0) end end",
			exy = expr("x", "y")
		),
	}
}

/// Shared format builder: the four comparison handlers (Lt/Le/Gt/Ge).
/// `swapped` = the metamethod receives (y, x) because Lua defines the
/// relation only for __lt/__le. Formats mirror bin_op_body.
pub(crate) fn cmp_body(
	fmt: u8,
	native: &str,
	mm: &str,
	swapped: bool,
	p: &mut StrPool,
) -> String {
	let number = p.lit("number");
	let string = p.lit("string");
	let meta = p.lit(mm);
	let msg = p.lit("attempt to compare ");
	let with = p.lit(" with ");
	let call = |x: &str, y: &str| {
		if swapped {
			format!("f({y}, {x})")
		} else {
			format!("f({x}, {y})")
		}
	};
	let nat = native;
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local y = V[c + 1]; if TYP(x) == {number} and TYP(y) == {number} then V[a + 1] = {nat} elseif TYP(x) == {string} and TYP(y) == {string} then V[a + 1] = {nat} else local f = mget(x, {meta}) or mget(y, {meta}); if f then V[a + 1] = {cxy} else ERR({msg} .. TYP(x) .. {with} .. TYP(y), 0) end end",
			cxy = call("x", "y")
		),
		1 => {
			// renamed temps: native expression re-targeted at u/w
			let nat_uw = native.replace('x', "u").replace('y', "w");
			format!(
				"local u = V[b + 1]; local w = V[c + 1]; if TYP(u) == {number} and TYP(w) == {number} then V[a + 1] = {nat_uw} elseif TYP(u) == {string} and TYP(w) == {string} then V[a + 1] = {nat_uw} else local f = mget(u, {meta}) or mget(w, {meta}); if f then V[a + 1] = {cuw} else ERR({msg} .. TYP(u) .. {with} .. TYP(w), 0) end end",
				cuw = call("u", "w")
			)
		}
		_ => format!(
			"local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if tx == {number} and ty == {number} then V[a + 1] = {nat} elseif tx == {string} and ty == {string} then V[a + 1] = {nat} else local f = mget(x, {meta}) or mget(y, {meta}); if f then V[a + 1] = {cxy} else ERR({msg} .. tx .. {with} .. ty, 0) end end",
			cxy = call("x", "y")
		),
	}
}
