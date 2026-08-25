//! Deterministic PRNG (Park-Miller minimal standard, mod 2^31-1).
//! Pure arithmetic — behavior identical across Lua 5.1 / Luau when ported,
//! and reproducible: same seed => same sequence.

pub struct Rng {
	state: u64,
}

const MOD: u64 = 2_147_483_647; // 2^31 - 1
const A: u64 = 48_271;

impl Rng {
	pub fn new(seed: u64) -> Rng {
		let mut s = seed % (MOD - 1);
		if s < 1 {
			s = 12_345;
		}
		Rng { state: s }
	}

	/// Next raw value in [1, MOD-1].
	///
	/// Split-multiply so both terms stay below 2^63:
	/// state = 256*hi + lo  =>  A*state = A*lo + A*hi*256.
	/// (The earlier version dropped the `* 256`, computing
	/// A*(lo+hi) mod M instead — a degenerate map whose cycle is only
	/// 231 states after ~4730 warmup: the ENTIRE obfuscation
	/// randomization cycle-locked. Verified with an exact bignum
	/// model; the split below is algebraically A*state mod M.)
	pub fn next_raw(&mut self) -> u64 {
		let lo = self.state % 256;
		let hi = (self.state - lo) / 256;
		let term1 = (A * lo) % MOD;
		let term2 = ((A * hi) % MOD * 256) % MOD;
		self.state = (term1 + term2) % MOD;
		if self.state == 0 {
			self.state = 1;
		}
		self.state
	}

	/// Float in (0, 1].
	pub fn random(&mut self) -> f64 {
		self.next_raw() as f64 / MOD as f64
	}

	/// Int in [min, max] inclusive.
	///
	/// Uses the FULL period via 128-bit multiplicative reduction —
	/// NEVER `next_raw() % span`: Park-Miller's LOW bits have short
	/// cycles (period ~2^k for mod 2^k), so low-bit mod small spans
	/// cycle through a tiny name/choice space. With a large reserved
	/// set (VM template: hundreds of names) a retry loop over such a
	/// short cycle loops forever (observed hang in gen_name).
	pub fn int(&mut self, min: i64, max: i64) -> i64 {
		if max < min {
			return min;
		}
		let span = (max - min + 1) as u64;
		min + (self.next_raw() as u128 * span as u128 / MOD as u128) as i64
	}

	pub fn pick<T: Clone>(&mut self, list: &[T]) -> T {
		list[self.int(0, list.len() as i64 - 1) as usize].clone()
	}

	pub fn shuffle<T: Clone>(&mut self, t: &mut Vec<T>) {
		for i in (1..t.len()).rev() {
			let j = self.int(0, i as i64) as usize;
			t.swap(i, j);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reproducible() {
		let mut a = Rng::new(42);
		let mut b = Rng::new(42);
		for _ in 0..1000 {
			assert_eq!(a.next_raw(), b.next_raw());
		}
	}

	#[test]
	fn range() {
		let mut r = Rng::new(7);
		for _ in 0..10000 {
			let v = r.int(3, 9);
			assert!((3..=9).contains(&v));
		}
	}
}
