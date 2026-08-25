// quick standalone check (not part of the build)
const MOD: u64 = 2_147_483_647;
const A: u64 = 48_271;
fn next_split(state: u64) -> u64 {
    let lo = state % 256;
    let hi = (state - lo) / 256;
    let term1 = (A * lo) % MOD;
    let term2 = ((A * hi) % MOD * 256) % MOD;
    let s = (term1 + term2) % MOD;
    if s == 0 { 1 } else { s }
}
fn main() {
    // compare with the naive (u128 exact) version
    let mut a = 12345u64;
    let mut b = 12345u64;
    for _ in 0..100 {
        a = next_split(a);
        b = ((A as u128 * b as u128) % (MOD as u128)) as u64;
        if a == 0 { a = 1; }
        assert_eq!(a, b);
    }
    println!("split == exact for 100 steps: OK");
    // brent for up to 2^20 steps to confirm no short cycle
    let mut power: u64 = 1;
    let mut lam: u64 = 1;
    let mut tortoise = 12345u64;
    let mut hare = next_split(12345);
    let mut steps = 0u64;
    while tortoise != hare && steps < 1 << 20 {
        if power == lam { tortoise = hare; power *= 2; lam = 0; }
        hare = next_split(hare);
        lam += 1;
        steps += 1;
    }
    if tortoise == hare {
        println!("SHORT CYCLE FOUND: lam={}", lam);
    } else {
        println!("no cycle within 2^20 steps: OK (steps={})", steps);
    }
}
