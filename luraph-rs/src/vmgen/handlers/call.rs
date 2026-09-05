//! Call — fixed-arg call with vararg append + nres result placement.
//!
//! Formats rename the base/args/results locals («B»/«A»/«O» tokens);
//! every other token is identical, so formats are semantics-preserving.

const LEGACY: &str = "local «B» = a + 1; local f = V[«B»]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nargs = b + off; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, b do «A»[off + i] = V[«B» + i] end; if d == 1 then for i = 1, vargc do «A»[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local «O», nout = callcap(fn, «A», nargs); local nres = c; lastbase = a + 1; lastn = nout; local wn = nout; if nres ~= 255 and nres > wn then wn = nres end; for i = 1, wn do V[«B» + i] = «O»[i] end";

const V15: &str = "local «B» = a + 1; local f = V[S[«B»]]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nargs = b + off; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, b do «A»[off + i] = V[S[«B» + i]] end; if d == 1 then for i = 1, vargc do «A»[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local «O», nout = callcap(fn, «A», nargs); local nres = c; lastbase = a + 1; lastn = nout; local wn = nout; if nres ~= 255 and nres > wn then wn = nres end; for i = 1, wn do local s = S[«B» + i]; if s then V[s] = «O»[i] else O[«B» + i] = «O»[i] end end";

pub fn code(fmt: u8) -> String {
	let (b, a, o) = match fmt {
		0 => ("base", "args", "out"),
		_ => ("bs", "av", "ov"),
	};
	LEGACY.replace("«B»", b).replace("«A»", a).replace("«O»", o)
}

pub fn code_v15(fmt: u8) -> String {
	let (b, a, o) = match fmt {
		0 => ("base", "args", "out"),
		_ => ("bs", "av", "ov"),
	};
	V15.replace("«B»", b).replace("«A»", a).replace("«O»", o)
}
