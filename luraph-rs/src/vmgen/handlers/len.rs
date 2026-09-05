//! Len — V[a] = #V[b] with __len fallback (5.1 hosts: probe-gated).

use crate::vmgen::strpool::StrPool;

/// f0 canonical (`HAS_LEN_META and mget(...)` gate); f1 expanded
/// nested-if form of the same gate.
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let len = p.lit("__len");
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local f = HAS_LEN_META and mget(x, {len}); if f then V[a + 1] = f(x) else V[a + 1] = #x end"
		),
		_ => format!(
			"local x = V[b + 1]; if HAS_LEN_META then local f = mget(x, {len}); if f then V[a + 1] = f(x) else V[a + 1] = #x end else V[a + 1] = #x end"
		),
	}
}
