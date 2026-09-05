//! Le — V[a] = (V[b] <= V[c]) with __le fallback.

use crate::vmgen::handlers::cmp_body;
use crate::vmgen::strpool::StrPool;

pub fn code(fmt: u8, p: &mut StrPool) -> String {
	cmp_body(fmt, "x <= y", "__le", false, p)
}
