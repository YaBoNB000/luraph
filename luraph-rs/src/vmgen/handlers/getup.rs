//! GetUp — V[a] = upvalue cell b's value.

/// f0 canonical; f1 cell through a renamed local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "local u = ups[b + 1]; V[a + 1] = u.v[u.i]".to_string(),
		_ => "local uc = ups[b + 1]; V[a + 1] = uc.v[uc.i]".to_string(),
	}
}
