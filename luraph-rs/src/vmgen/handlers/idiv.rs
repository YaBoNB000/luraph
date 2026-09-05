//! Idiv — floor division (5.1-compatible: FLOOR(x / y)).

/// f0 canonical inline; f1 quotient through a local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "V[a + 1] = FLOOR(V[b + 1] / V[c + 1])".to_string(),
		_ => "local q = V[b + 1] / V[c + 1]; V[a + 1] = FLOOR(q)".to_string(),
	}
}
