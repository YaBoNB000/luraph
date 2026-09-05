//! Nop — dead instruction (padding + self-modification alias target).
//!
//! The body is a harmless arithmetic local; the shape is drawn per
//! build (kept here so every instruction's code lives in its own file).

use crate::rng::Rng;

pub fn body(rng: &mut Rng) -> String {
	match rng.int(0, 2) {
		0 => "local _ = a + b".to_string(),
		1 => "local _ = c * d".to_string(),
		_ => "local _ = a * c + b".to_string(),
	}
}
