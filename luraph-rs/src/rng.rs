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
	pub fn next_raw(&mut self) -> u64 {
		// (state * A) mod MOD, split to stay within u64 exact range
		let lo = self.state % 256;
		let hi = (self.state - lo) / 256;
		self.state = (A * lo + (A * hi) % MOD) % MOD;
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
	pub fn int(&mut self, min: i64, max: i64) -> i64 {
		if max < min {
			return min;
		}
		let span = max - min + 1;
		min + (self.next_raw() % span as u64) as i64
	}

	pub fn pick<T: Clone>(&mut self, list: &[T]) -> T {
		list[(self.next_raw() % list.len() as u64) as usize].clone()
	}

	pub fn shuffle<T: Clone>(&mut self, t: &mut Vec<T>) {
		for i in (1..t.len()).rev() {
			let j = (self.next_raw() % (i as u64 + 1)) as usize;
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
