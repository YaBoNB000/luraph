//! SetUp — upvalue cell a := V[b].

/// f0 canonical; f1 cell through a renamed local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "local u = ups[a + 1]; u.v[u.i] = V[b + 1]".to_string(),
		_ => "local uc = ups[a + 1]; uc.v[uc.i] = V[b + 1]".to_string(),
	}
}
