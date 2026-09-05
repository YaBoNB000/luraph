//! CallT — call appending results into a table constructor slot.
//!
//! Formats rename args/target/count/results locals («A»/«T»/«N»/«O»).

const LEGACY: &str = "local f = V[a + 1]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = FLOOR(d / 2); local tail = d % 2 == 1; local ntail = tail and V[a + nfixed + 2] or 0; local nargs = nfixed + off + ntail; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, nfixed + ntail do if tail and i > nfixed then «A»[off + i] = V[a + i + 2] else «A»[off + i] = V[a + i + 1] end end; local «T» = V[b + 1]; local «N» = V[c + 1]; local «O», nout = callcap(fn, «A», nargs); for i = 1, nout do «T»[«N» + i] = «O»[i] end; V[c + 1] = «N» + nout";

const V15: &str = "local f = V[S[a + 1]]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = FLOOR(d / 2); local tail = d % 2 == 1; local ntail = tail and V[S[a + nfixed + 2]] or 0; local nargs = nfixed + off + ntail; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, nfixed + ntail do if tail and i > nfixed then local x = a + i + 2; local s = S[x]; if s then «A»[off + i] = V[s] else «A»[off + i] = O[x] end else «A»[off + i] = V[S[a + i + 1]] end end; local «T» = V[b + 1]; local «N» = V[c + 1]; local «O», nout = callcap(fn, «A», nargs); for i = 1, nout do «T»[«N» + i] = «O»[i] end; V[c + 1] = «N» + nout";

pub fn code(fmt: u8) -> String {
	let (a, t, n, o) = match fmt {
		0 => ("args", "t", "n", "out"),
		_ => ("av", "tb", "n0", "ov"),
	};
	LEGACY
		.replace("«A»", a)
		.replace("«T»", t)
		.replace("«N»", n)
		.replace("«O»", o)
}

pub fn code_v15(fmt: u8) -> String {
	let (a, t, n, o) = match fmt {
		0 => ("args", "t", "n", "out"),
		_ => ("av", "tb", "n0", "ov"),
	};
	V15.replace("«A»", a)
		.replace("«T»", t)
		.replace("«N»", n)
		.replace("«O»", o)
}
