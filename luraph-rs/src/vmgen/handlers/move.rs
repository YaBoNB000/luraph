//! Move — register copy V[a] = V[b].

/// f0 canonical; f1 value through a local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "V[a + 1] = V[b + 1]".to_string(),
		_ => "local t = V[b + 1]; V[a + 1] = t".to_string(),
	}
}
