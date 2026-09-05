//! LoadK — load constant C[b] into V[a].

/// f0 canonical; f1 constant through a local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "V[a + 1] = C[b + 1]".to_string(),
		_ => "local t = C[b + 1]; V[a + 1] = t".to_string(),
	}
}
