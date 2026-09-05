//! CallE — call with FULL-EXPANSION results (multi-return staging).
//!
//! f at V[a+1], b fixed args; d&1 appends varargs, d>=2 consumes a
//! tail staged by a nested CallE. Results → V[a+2..]; the RESULT COUNT
//! lands in V[a+1]. Formats rename base/args/results locals only.

const LEGACY: &str = "local «B» = a + 1; local f = V[«B»]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = b; local varg = d % 2 == 1; local tail = d >= 2; local nargs = nfixed + off; if tail then nargs = nargs + V[«B» + nfixed + 1] end; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, (tail and (nfixed + V[«B» + nfixed + 1]) or nfixed) do if tail and i > nfixed then «A»[off + i] = V[«B» + i + 1] else «A»[off + i] = V[«B» + i] end end; if varg then for i = 1, vargc do «A»[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local «O», nout = callcap(fn, «A», nargs); for i = 1, nout do V[«B» + i] = «O»[i] end; V[«B»] = nout";

const V15: &str = "local «B» = a + 1; local f = V[S[«B»]]; local fn, selfv = resolve_call(f); local off = selfv and 1 or 0; local nfixed = b; local varg = d % 2 == 1; local tail = d >= 2; local nargs = nfixed + off; if tail then nargs = nargs + V[S[«B» + nfixed + 1]] end; local «A» = {}; if off == 1 then «A»[1] = f end; for i = 1, (tail and (nfixed + V[S[«B» + nfixed + 1]]) or nfixed) do if tail and i > nfixed then local x = «B» + i + 1; local s = S[x]; if s then «A»[off + i] = V[s] else «A»[off + i] = O[x] end else «A»[off + i] = V[S[«B» + i]] end end; if varg then for i = 1, vargc do «A»[nargs + i] = vargs[i] end; nargs = nargs + vargc end; local «O», nout = callcap(fn, «A», nargs); for i = 1, nout do local s = S[«B» + i]; if s then V[s] = «O»[i] else O[«B» + i] = «O»[i] end end; V[S[«B»]] = nout";

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
