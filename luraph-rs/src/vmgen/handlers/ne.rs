//! Ne — V[a] = (V[b] ~= V[c]) via the Eq logic, negated.

use crate::vmgen::strpool::StrPool;

/// f0 canonical; f1 renamed temporaries (u/w/tu/tw).
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let number = p.lit("number");
	let string = p.lit("string");
	let boolean = p.lit("boolean");
	let nil = p.lit("nil");
	let eq = p.lit("__eq");
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); local eqv; if tx == ty and (tx == {number} or tx == {string} or tx == {boolean} or tx == {nil}) then eqv = x == y else local f = mget(x, {eq}) or mget(y, {eq}); if f then eqv = f(x, y) else eqv = x == y end end; V[a + 1] = not eqv"
		),
		_ => format!(
			"local u = V[b + 1]; local w = V[c + 1]; local tu = TYP(u); local tw = TYP(w); local eqv; if tu == tw and (tu == {number} or tu == {string} or tu == {boolean} or tu == {nil}) then eqv = u == w else local f = mget(u, {eq}) or mget(w, {eq}); if f then eqv = f(u, w) else eqv = u == w end end; V[a + 1] = not eqv"
		),
	}
}
