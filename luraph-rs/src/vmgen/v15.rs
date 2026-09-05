//! L6 VM — v15 structural-parity emitter support (Route A, P2).
//!
//! Builds the Luraph-v15-style module-table fields that surround the
//! interpreter (docs/v15-structural-parity-plan.md):
//!
//! - 65 primitive numeric slots (buffer/bit32/string/table/coroutine/...),
//!   slot numbers Fisher-Yates'd over 1..=126 per build;
//! - 2 in-table LCG factory slots (decoy PRNGs: statically uncalled,
//!   mod 2^28 state writes, sample [63]/[96] shape, F15/F28);
//! - 1 mutable constant slot `[N]=(0)` (runtime-mutated in later phases);
//! - named constant fields (AL/BL/EL/PL/QL/cL/dL/gL/h/s — sample shape);
//! - boot handler name from the v15 alphabet scheme (base + `C` suffix,
//!   entry-machine family, sample `FC`).
//!
//! Vector3.new / Vector2.new are NOT emitted: Roblox-only globals, absent
//! from the Luau CLI, and table construction evaluates every RHS. The
//! dual-target product constraint is documented in the parity plan §2;
//! `vector.create` covers fingerprint F19.
//!
//! Fields land in the table AFTER the interpreter block's obfuscation
//! passes (main.rs), so their constants/names stay verbatim like the
//! sample's (P2 acceptance: buffer./bit32. only on table-init RHS).

use crate::ast::{self, Expr, TableField};
use crate::parser;
use crate::rng::Rng;
use crate::vmgen::isa::CARRIER_SPECIALS;

/// (value source, is_dot) primitives. Dots = `lib.name`, bare = global.
const PRIM_DOTS: &[&str] = &[
	// buffer (23 — buffer.fill/create/read*/write*/copy/len/tostring/
	// fromstring/readstring)
	"buffer.fill",
	"buffer.create",
	"buffer.len",
	"buffer.copy",
	"buffer.tostring",
	"buffer.fromstring",
	"buffer.readstring",
	"buffer.readu8",
	"buffer.readu16",
	"buffer.readi16",
	"buffer.readu32",
	"buffer.readi32",
	"buffer.readf32",
	"buffer.readf64",
	"buffer.writeu8",
	"buffer.writei8",
	"buffer.writeu32",
	// bit32 (7)
	"bit32.bxor",
	"bit32.band",
	"bit32.bor",
	"bit32.bnot",
	"bit32.lshift",
	"bit32.rshift",
	"bit32.countrz",
	// string (11)
	"string.byte",
	"string.char",
	"string.sub",
	"string.rep",
	"string.find",
	"string.match",
	"string.gmatch",
	"string.gsub",
	"string.format",
	"string.pack",
	"string.unpack",
	// table (5)
	"table.create",
	"table.pack",
	"table.insert",
	"table.concat",
	"table.move",
	// coroutine (8)
	"coroutine.create",
	"coroutine.resume",
	"coroutine.yield",
	"coroutine.wrap",
	"coroutine.status",
	"coroutine.running",
	"coroutine.close",
	"coroutine.isyieldable",
	// Roblox-ish value constructors present in the Luau CLI
	"vector.create",
];

const PRIM_BARE: &[&str] = &[
	"typeof", "setfenv", "getfenv", "pcall", "xpcall", "error", "next",
	"select", "unpack", "tonumber", "tostring", "rawget", "rawset",
	"assert", "type", "setmetatable", "getmetatable",
];

/// 53-base alphabet of the v15 naming scheme (sample: A–Z / `_` / a–z).
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

fn dot_value(path: &str) -> Expr {
	let (lib, name) = path.split_once('.').unwrap();
	Expr::Dot {
		obj: Box::new(Expr::Ident {
			name: lib.to_string(),
			sym: None,
		}),
		name: name.to_string(),
	}
}

/// Boot handler name: base letter + `C` suffix (entry-machine family,
/// sample `FC`). 2 chars keeps fingerprint F21's len<=2 scheme.
pub fn boot_name(rng: &mut Rng) -> String {
	let base = ALPHA[rng.int(0, 52) as usize] as char;
	format!("{}C", base)
}

/// Parse a single Lua expression out of `local _ = <expr>`.
/// (On failure the generated source is dumped — these are static
/// build-time strings, a failure here is an emitter bug.)
fn parse_expr(src: &str) -> Expr {
	let chunk = format!("local _ = {}", src);
	let mut block = match parser::parse(&chunk, true) {
		Ok(b) => b,
		Err(e) => {
			eprintln!("v15 emitter bug: {:?}\nsource:\n{}", e, chunk);
			panic!("v15 static source")
		}
	};
	match block.stmts.pop().unwrap() {
		ast::Stmt::Local { mut values, .. } => values.pop().unwrap().unwrap(),
		_ => unreachable!(),
	}
}

/// One LCG factory slot value: `function(<shadowed params>) return
/// function() ... b[1][4][b[1][7]] = (M*x+C) % 2^28 steps ... end end`.
/// Params are deliberately shadowed (sample `[63]=function(b,b,b,u)`,
/// fingerprint F25). Never called anywhere (decoy, F28).
fn lcg_factory(rng: &mut Rng, states: usize) -> Expr {
	let cell = "b[1][4][b[1][7]]";
	let step = |rng: &mut Rng, cell: &str| -> String {
		let m = rng.int(100_001, 1_100_001);
		let c = rng.int(1_000_000, 268_000_000);
		format!("({m}*{cell}+{c})%268435456", m = m, cell = cell, c = c)
	};
	let mut body = String::from("local u=0;while true do ");
	for s in 0..states {
		if s == 0 {
			body.push_str("if u<=0 then ");
		} else {
			body.push_str(&format!("elseif u<={} then ", s));
		}
		// step 1: plain state rewrite
		body.push_str(&format!("{cell}={v};", cell = cell, v = step(rng, cell)));
		// step 2: rewrite + advance the state counter
		body.push_str(&format!(
			"{cell},u={v},{ns};",
			cell = cell,
			v = step(rng, cell),
			ns = s + 1
		));
	}
	body.push_str("else return;end;end");
	let params = if states >= 2 { "b,b,b,u" } else { "b,b" };
	let src = format!(
		"function({params})return function() {body} end end",
		params = params,
		body = body
	);
	parse_expr(&src)
}

/// All P2 module-table fields (primitives + LCG factories + mutable
/// constant slot + named constants). Slot numbers are a Fisher-Yates
/// sample of 1..=126 (sparse, per build).
pub fn module_fields(rng: &mut Rng, exclude: &[i64]) -> Vec<TableField> {	let mut slots: Vec<i64> = (1..=126)
		.filter(|n| !exclude.contains(n))
		.collect();
	rng.shuffle(&mut slots);
	let mut it = slots.into_iter();
	let mut fields: Vec<TableField> = Vec::new();

	for path in PRIM_DOTS.iter().chain(PRIM_BARE.iter()) {
		let n = it.next().unwrap();
		let value = if path.contains('.') {
			dot_value(path)
		} else {
			Expr::Ident {
				name: path.to_string(),
				sym: None,
			}
		};
		fields.push(TableField::Key {
			key: Expr::Num {
				value: n as f64,
				isfloat: false,
			},
			value,
		});
	}
	// 2 LCG factory slots (2-state and 3-state machines)
	for states in [2usize, 3usize] {
		let n = it.next().unwrap();
		fields.push(TableField::Key {
			key: Expr::Num {
				value: n as f64,
				isfloat: false,
			},
			value: lcg_factory(rng, states),
		});
	}
	// 1 mutable constant slot: `[N]=(0)` (later phases mutate it)
	{
		let n = it.next().unwrap();
		fields.push(TableField::Key {
			key: Expr::Num {
				value: n as f64,
				isfloat: false,
			},
			value: Expr::Num {
				value: 0.0,
				isfloat: false,
			},
		});
	}

	// Named constant fields (sample shape): AL/BL/EL/PL/QL/cL/dL/gL/h/s
	let named_str = |name: &str, val: &str| TableField::Key {
		key: Expr::Str {
			bytes: name.as_bytes().to_vec(),
			is_binary: false,
		},
		value: Expr::Str {
			bytes: val.as_bytes().to_vec(),
			is_binary: false,
		},
	};
	fields.push(named_str("AL", "n"));
	fields.push(named_str("BL", "__index"));
	fields.push(named_str("EL", ": "));
	fields.push(named_str("PL", "?"));
	fields.push(named_str("cL", "string"));
	fields.push(named_str("dL", "LPH:"));
	// gL = line-number rewrite regex (nL consumes it in P8)
	fields.push(TableField::Key {
		key: Expr::Str {
			bytes: b"gL".to_vec(),
			is_binary: false,
		},
		value: Expr::Str {
			bytes: b":(%d+)[:\r\n]".to_vec(),
			is_binary: false,
		},
	});
	fields.push(TableField::Key {
		key: Expr::Str {
			bytes: b"QL".to_vec(),
			is_binary: false,
		},
		value: Expr::Bool { value: false },
	});
	// s = {} (scratch/env table passed to the entry maker in P6)
	fields.push(TableField::Key {
		key: Expr::Str {
			bytes: b"s".to_vec(),
			is_binary: false,
		},
		value: Expr::Table { fields: Vec::new() },
	});
	// h = {24123, 8 random u32s} — dead constant table (decoy, F28)
	{
		let mut elems = vec![Expr::Num {
			value: 24123.0,
			isfloat: false,
		}];
		for _ in 0..8 {
			elems.push(Expr::Num {
				value: rng.int(0, 4_294_967_295) as f64,
				isfloat: false,
			});
		}
		fields.push(TableField::Key {
			key: Expr::Str {
				bytes: b"h".to_vec(),
				is_binary: false,
			},
			value: Expr::Table { fields: elems.into_iter().map(TableField::Array).collect() },
		});
	}

	rng.shuffle(&mut fields);
	fields
}

/// Binary-safe Lua string literal (`\ddd` for anything outside
/// printable ASCII; always 3-digit escapes so `\5` + `1` can never
/// merge into `\51`).
fn lua_bytes_lit(bytes: &[u8]) -> String {
	let mut s = String::with_capacity(bytes.len() * 4 + 2);
	s.push('"');
	for &b in bytes {
		match b {
			b'"' => s.push_str("\\\""),
			b'\\' => s.push_str("\\\\"),
			32..=126 => s.push(b as char),
			_ => s.push_str(&format!("\\{:03}", b)),
		}
	}
	s.push('"');
	s
}

/// Name allocator over the v15 alphabet: bare singles, `xL` (machine /
/// assistant family), `xC` (entry-machine family, sample `FC`).
/// Never collides with the P2 constant field names.
struct Names {
	pool: Vec<String>,
	/// exhaustion fallback counter (fresh namespace the pool never
	/// generates — large corpora drain the shuffled pool)
	fallback: usize,
}

impl Names {
	fn new(rng: &mut Rng) -> Names {
		let mut pool = Vec::new();
		let singles: Vec<&str> = vec![
			"a", "c", "d", "e", "f", "g", "i", "j", "k", "l", "m", "n",
			"o", "p", "q", "r", "t", "u", "v", "w", "x", "y", "z", "A",
			"B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
			"N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y",
			"Z", "_",
		];
		for b in singles.iter() {
			pool.push(b.to_string());
			pool.push(format!("{}L", b));
			pool.push(format!("{}C", b));
		}
		for a in singles.iter() {
			for b in singles.iter() {
				pool.push(format!("{}{}", a, b));
			}
		}
		rng.shuffle(&mut pool);
		// P2 constant fields + self param are reserved; two-letter
		// Lua keywords banned outright (keyword-as-name = parse error).
		// Dedup too: the singles loop's `{x}C`/`{x}L` forms collide with
		// two-letter combos (e.g. "f"+"C" == "fC") — a duplicate pool
		// entry hands the same name out twice and two module-table fields
		// end up sharing a key (later wins -> a staging handler silently
		// disappears -> state chain loops forever).
		let reserved = [
			"AL", "BL", "EL", "PL", "QL", "XC", "cL", "dL", "gL", "RC",
			"pC", "h", "s", "b", "do", "if", "in", "or",
		];
		pool.retain(|n| !reserved.contains(&n.as_str()));
		// coded_name() emits `{var}t` / `{var}o` / `{var}i` for each
		// drawn name — a name whose SUFFIXED derivative is a keyword
		// parses as that keyword (e.g. var "no" -> "not"). Ban any pool
		// entry whose own or suffixed form is a Lua/Luau keyword.
		const KW: &[&str] = &[
			"and", "break", "do", "else", "elseif", "end", "false", "for",
			"function", "if", "in", "local", "nil", "not", "or", "repeat",
			"return", "then", "true", "until", "while", "continue",
		];
		pool.retain(|n| {
			!KW.contains(&n.as_str())
				&& !KW.contains(&(n.clone() + "t").as_str())
				&& !KW.contains(&(n.clone() + "o").as_str())
				&& !KW.contains(&(n.clone() + "i").as_str())
		});
		let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
		pool.retain(|n| seen.insert(n.clone()));
		Names { pool, fallback: 0 }
	}
	fn take(&mut self) -> String {
		match self.pool.pop() {
			Some(n) => n,
			None => {
				self.fallback += 1;
				format!("vN{}", self.fallback)
			}
		}
	}
	/// Draw a name that is not in `forbidden` — forbidden pops are
	/// stashed and returned to the pool afterwards (they stay valid
	/// names elsewhere). Needed inside scaffold handler bodies, whose
	/// fixed parameter/local names (C, E, i, Q, ...) must never be
	/// shadowed by a generated local — shadowing the context param `C`
	/// turns `C[aux]=s` into indexing a function.
	fn take_avoid(&mut self, forbidden: &[&str]) -> String {
		let mut stashed: Vec<String> = Vec::new();
		let result = loop {
			match self.pool.pop() {
				Some(n) if !forbidden.contains(&n.as_str()) => break n,
				Some(n) => stashed.push(n),
				None => {
					self.fallback += 1;
					break format!("vN{}", self.fallback);
				}
			}
		};
		self.pool.extend(stashed);
		result
	}
}

/// Binary dispatch tree over sorted (state, leaf) pairs (sample style:
/// `if V<=N then ... else ...` range tree).
/// Suggestion 2 — opaque predicate wrapping. Wraps a dispatch leaf in
/// `if <opaque> then REAL else DECOY end` (or the inverted direction),
/// where the predicate is arithmetically always-true on the numeric
/// state `V` but not obviously so. Static analysis cannot tell which
/// branch runs without proving the predicate; the decoy branch is dead.
/// The direction and predicate are randomized per leaf per build.
fn opaque_wrap(code: String, rng: &mut crate::rng::Rng, vname: &str) -> String {
	// 增量⑪ (防静态, 报告突破口 #3): the old predicates (v-v==0,
	// v*v>=0, v%2==0 or v%2~=0) were OBVIOUS tautologies the attacker
	// folded to statically simulate the whole state machine. Replace with
	// less-obvious identities that still hold for every (non-negative
	// integer) state value but require actual reasoning to prove
	// always-true, raising the cost of static branch collapsing.
	let preds: [String; 4] = [
		// v(v+1) is always even (product of consecutive integers)
		format!("((({v}*{v})+{v})%2)==0", v = vname),
		// a square is never negative
		format!("(({v}%3)*({v}%3))>=0", v = vname),
		// (v+1)^2 = v^2+2v+1 >= v^2 for v>=0
		format!("(({v}+1)*({v}+1))>=({v}*{v})", v = vname),
		// (v%5)^4 is non-negative
		format!("((({v}%5)*({v}%5))*(({v}%5)*({v}%5)))>=0", v = vname),
	];
	let p = &preds[rng.int(0, 3) as usize];
	if rng.int(0, 1) == 0 {
		format!("if {} then {} else local _={} end", p, code, vname)
	} else {
		format!("if not({}) then local _={} else {} end", p, vname, code)
	}
}

fn dispatch_tree(
	vname: &str,
	leaves: &[(i64, String)],
	lo: usize,
	hi: usize,
) -> String {
	if hi - lo == 1 {
		return leaves[lo].1.clone();
	}
	let mid = (lo + hi) / 2;
	let thr = leaves[mid - 1].0;
	format!(
		"if {}<={} then {} else {} end",
		vname,
		thr,
		dispatch_tree(vname, leaves, lo, mid),
		dispatch_tree(vname, leaves, mid, hi)
	)
}

/// P3 increment 1 — CPS bootstrap scaffold (sample shape):
/// initializer + top machine + CPS loop with `continue` leaves +
/// per-carrier staging handlers + interpreter-definition handler +
/// control-code handler + mul32/mod2^32 arithmetic assistants.
///
/// `interp_src` is the already-passed interpreter chunk (defines the
/// `vm_name` local); `carriers` are the encoded bytecode strings.
/// Returns module-table fields (as parsed expressions) + the entry
/// machine's field name.
/// Chunk literals per carrier (sample-scale staging chain density:
/// grows named handlers / state returns / fused conditionals).
pub const CHUNKS: usize = 5;

/// P3b: max u32 words per numeric-slot word array (kept under the
/// attack's "large numeric literal" threshold; blends into the
/// numeric-slot family).
pub const BW_SLOT_WORDS: usize = 90;

/// Decoy LCG slots (F28): parked far above BOTH the context C range
/// (..=auxslot) and the primitive-slot draw (1..=126), and shifted off
/// any reserved runner/keystream slot, so no other mechanism ever
/// indexes them -> static external refs stay 0 and no slot collision
/// can shadow a runner (2026-08-29 seed-42 tables.lua regression).
/// Decoy LCG slots (F28). Slot numbers must clear every numeric-index
/// mechanism in the output: module-field draw (1..126), runner/keystream
/// slots, primitive slots (1..80), AL alphabet keys (byte values 32..126),
/// context C slots (..=auxslot) -- and, critically, the Nop self-mod
/// literal writes `ZW[pos] = alias` whose positions reach the per-function
/// instruction counts (bounded by the carrier byte lengths). Parking the
/// decoys above max carrier length + margin clears all of them.
pub fn decoy_slots(max_carrier_len: usize, avoid: &[i64]) -> (i64, i64) {
	let mut d1 = max_carrier_len as i64 + 500;
	let mut d2 = d1 + 1;
	while avoid.contains(&d1) {
		d1 += 2;
	}
	while avoid.contains(&d2) || d2 == d1 {
		d2 += 2;
	}
	(d1, d2)
}

/// P4 (防御代码隐藏): emit a runtime string-builder for `name` —
/// char codes stored SHUFFLED in a numeric table plus an order list,
/// concatenated through the module's string.char slot. The name never
/// appears in the output (codes look like arbitrary slot arithmetic).
/// Appends declarations to `out`; returns the result variable name.
fn coded_name(out: &mut String, var: &str, name: &str, sc_slot: i64, rng: &mut Rng) {
	let codes: Vec<u8> = name.bytes().collect();
	let mut pos: Vec<usize> = (0..codes.len()).collect();
	rng.shuffle(&mut pos);
	out.push_str(&format!("local {var}t={{}};"));
	for (i, &c) in codes.iter().enumerate() {
		out.push_str(&format!("{var}t[{}]={c};", pos[i] + 1));
	}
	// order[k] = storage index holding output char k
	let mut inv = vec![0usize; codes.len()];
	for (i, &p) in pos.iter().enumerate() {
		inv[i] = p + 1;
	}
	out.push_str(&format!("local {var}o={{"));
	out.push_str(&inv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
	out.push_str(&format!(
		"}};local {var}=\"\";for {var}i=1,#{var}o do {var}={var}..b[{sc}]({var}t[{var}o[{var}i]]) end;",
		sc = sc_slot
	));
}

/// P5 (staging 去重复): per-build random names for the CPS state
/// registers (were fixed V/f1..f5 — the most repeated literal tuple in
/// the output, ~100 identical call sites). Must avoid the entry
/// machine's fixed locals so no accidental shadowing rewrites the state.
fn draw_state_names(nm: &mut Names) -> [String; 6] {
	const FIXED: &[&str] = &[
		"b", "C", "u", "z", "Z", "o", "w", "K", "G", "q", "M", "F", "H",
		"E", "J", "B",
	];
	let mut forbid: Vec<String> = FIXED.iter().map(|s| s.to_string()).collect();
	let mut out: Vec<String> = Vec::new();
	for _ in 0..6 {
		let fs: Vec<&str> = forbid.iter().map(|s| s.as_str()).collect();
		let n = nm.take_avoid(&fs);
		forbid.push(n.clone());
		out.push(n);
	}
	out.try_into().unwrap()
}

pub fn scaffold(
	rng: &mut Rng,
	interp_src: &str,
	vm_name: &str,
	carriers: &[Vec<u8>],
	runner1: i64,
	runner2: i64,
	ks_slot: i64,
	kg_slot: i64,
	decoy1: i64,
	decoy2: i64,
	carrier: &crate::vmgen::isa::Carrier,
	guard: bool,
	bw_slots: &[i64],
	prim_extra: (i64, i64),
	// 增量⑩: key-fragment slots. kfrag[0] = second ks-seed fragment;
	// kfrag[1..=4] = the bm0/bc0 word-mask key fragments. All blend
	// into the numeric-slot family (values are random halves, not keys).
	kfrag: &[i64],
) -> (Vec<TableField>, String) {
	let carrier_tokens: &[String] = &carrier.tokens;
	let mut nm = Names::new(rng);
	// P5: per-build state-register names (see draw_state_names)
	let state = draw_state_names(&mut nm);
	let sv = state[0].clone();
	let rhs5 = format!(
		"{},{},{},{},{}",
		state[1], state[2], state[3], state[4], state[5]
	);
	let lhs6 = format!("{},{}", sv, rhs5);
	let leaf = |name: &str| {
		format!("{lhs6}=b:{name}(C,{rhs5});continue;")
	};
	let fc = nm.take(); // entry machine
	let init = nm.take(); // initializer
	let xl = nm.take(); // sub-dispatcher (FC -> XL -> CPS loop)
	let init2 = nm.take(); // extra machine initializer (sample D/J/K + KL/M/zL family)
	let init3 = nm.take();
	let mul = nm.take(); // mul32 assistant
	let modulo = nm.take(); // mod 2^32 assistant
	let loop_name = nm.take(); // CPS loop
	let ctl = nm.take(); // control handler
	let ehandler = nm.take(); // entry-builder handler
	let v1h = nm.take(); // verify (re-fold over stored chunks)
	let v2h = nm.take(); // verify (compare -> trap / continue)

	let n = carriers.len();

	// SECURITY (sample parity): the carrier keystream comes from a
	// state-machine LCG, NOT literal constants in the chunk handlers.
	// kg_slot holds a 3-step LCG generator (sample [96] shape) that
	// advances the state and returns it. Each chunk handler calls the
	// generator to obtain its key constant, so no key material appears
	// at the decode site.
	// 增量⑩ (防静态, 报告突破口 #5 — R001 直接读走了 [ks]=种子 字面量):
	// the seed is SPLIT into two meaningless fragments parked at random
	// module slots; the generator ASSEMBLES the real seed on its first
	// call (state := fragA + fragB, then the marker slot is erased).
	// The LCG constants themselves are emitted as two-term arithmetic,
	// so no key value survives as a single literal anywhere.
	let ks_seed: i64 = rng.int(1_048_576, 268_435_455);
	let lcm = |rng: &mut Rng| rng.int(1_048_577, 33_000_001) | 1;
	let lcc = |rng: &mut Rng| rng.int(1_048_576, 268_000_000) | 1;
	let (km1, kc1) = (lcm(rng), lcc(rng));
	let (km2, kc2) = (lcm(rng), lcc(rng));
	let (km3, kc3) = (lcm(rng), lcc(rng));
	crate::vmgen::manifest_key("KS_SEED", ks_seed as u64);
	crate::vmgen::manifest_key("KG_M1", km1 as u64);
	crate::vmgen::manifest_key("KG_C1", kc1 as u64);
	crate::vmgen::manifest_key("KG_M2", km2 as u64);
	crate::vmgen::manifest_key("KG_C2", kc2 as u64);
	crate::vmgen::manifest_key("KG_M3", km3 as u64);
	crate::vmgen::manifest_key("KG_C3", kc3 as u64);
	let mut ks_state: i64 = ks_seed;
	// 增量⑩: split a value into an `(a+b)` two-term sum (no bare literal).
	let split_sum = |v: i64, rng: &mut Rng| -> String {
		let a = rng.int(1, v - 1);
		format!("({}+{})", a, v - a)
	};
	// 增量⑩: store `v` in a table field as a `(u-d)` arithmetic
	// expression (never a bare number). d stays 8-digit so u fits a
	// double with room to spare.
	let split_field = |v: i64, rng: &mut Rng| -> Expr {
		let d = rng.int(10_000_000, 99_999_999);
		parse_expr(&format!("({}-{})", v + d, d))
	};
	let ks_frag_a = rng.int(0, 268_435_455);
	let ks_frag_b = (ks_seed - ks_frag_a).rem_euclid(268_435_456);

	// ---- RT reverse-token table names (F8 pC shape below)
	let rt_name = nm.take();

	// Split every carrier into CHUNKS literals (the entry builder
	// concats them back when calling the VM).
	let chunked: Vec<Vec<Vec<u8>>> = carriers
		.iter()
		.map(|c| {
			let base = c.len() / CHUNKS;
			let rem = c.len() % CHUNKS;
			let mut out = Vec::new();
			let mut s = 0;
			for i in 0..CHUNKS {
				let len = base + if i < rem { 1 } else { 0 };
				out.push(c[s..s + len].to_vec());
				s += len;
			}
			out
		})
		.collect();

	// context-table layout:
	//   C[1 .. n*CHUNKS]  carrier chunks
	//   C[vmslot]         VM closure (set by the def handler)
	//   C[entryslot]      entry closure
	//   C[sumslot]        fold checksum (staging phase)
	//   C[verifyslot]     re-fold checksum (verify phase)
	let vmslot = n * CHUNKS + 1;
	let entryslot = vmslot + 1;
	let sumslot = entryslot + 1;
	let verifyslot = sumslot + 1;
	let auxslot = verifyslot + 1;

	// Scale floor (F4/F5/F6/F21/F23): small source programs yield few
	// staging handlers; pad the chain with real state-advancing
	// handlers up to sample-scale density so every output carries the
	// full fingerprint magnitude.
	const MIN_STAGING: usize = 100;
	let natural = n * (1 + CHUNKS);
	let filler = if natural < MIN_STAGING { MIN_STAGING - natural } else { 0 };
	// one state per step + terminal; ascending
	let nsteps = natural + filler + 4;
	let need = (nsteps + 1) as i64;
	let mut ids: Vec<i64> = (3..=236).collect();
	if (ids.len() as i64) < need {
		ids.extend(237..=236 + need);
	}
	rng.shuffle(&mut ids);
	let mut states: Vec<i64> = ids[..need as usize].to_vec();
	states.sort();
	let sdone = states[nsteps as usize];

	let mut fields: Vec<TableField> = Vec::new();
	let mut leaves: Vec<(i64, String)> = Vec::new();
	let named = |name: String, value: Expr| TableField::Key {
		key: Expr::Str {
			bytes: name.into_bytes(),
			is_binary: false,
		},
		value,
	};
	let slotted = |slot: i64, value: Expr| TableField::Key {
		key: Expr::Num {
			value: slot as f64,
			isfloat: false,
		},
		value,
	};
	// SECURITY: LCG keystream slots. KSC = the LCG state; KG = a 3-step
	// LCG state-machine generator (sample [96] shape) that advances
	// b[KSC] by three transitions and returns it.
	// 增量⑩: b[KSC] initially holds fragment A of the seed (arithmetic
	// expression, not the seed); fragment B parks at kfrag[0]. On the
	// FIRST generator call the state is assembled (A+B), written back
	// and the marker slot erased — from then on the generator behaves
	// exactly as before.
	fields.push(slotted(ks_slot, split_field(ks_frag_a, rng)));
	fields.push(slotted(kfrag[0], split_field(ks_frag_b, rng)));
	// P3b: operand order emitted as (x*m+c) and per-step local names
	// vary — the keystream stays runtime-derived (sample [96] family)
	// but no fixed-shape constant triple is grep-able in one function.
	// 增量⑩: m/c constants emitted as two-term sums as well.
	fields.push(slotted(
		kg_slot,
		parse_expr(&format!(
			"function(b) local x=b[{ks}];if b[{fb}]~=nil then x=(x+b[{fb}])%268435456;b[{ks}]=x;b[{fb}]=nil end;local u=0;while true do if u<=0 then x=(x*{m1}+{c1})%268435456;u=1 elseif u<=1 then local y=(x*{m2}+{c2})%268435456;x=y;u=2 else local z=(x*{m3}+{c3})%268435456;b[{ks}]=z;return z end end end",
			ks = ks_slot,
			fb = kfrag[0],
			m1 = split_sum(km1, rng), c1 = split_sum(kc1, rng),
			m2 = split_sum(km2, rng), c2 = split_sum(kc2, rng),
			m3 = split_sum(km3, rng), c3 = split_sum(kc3, rng),
		)),
	));
	// wide state tuple: handlers take (b, C, p1..p5) = 7 params (F6)
	let mut fillers: Vec<String> = ["p1", "p2", "p3", "p4", "p5"].iter().map(|s| s.to_string()).collect();
	let mut state_i = 0usize;
	// (dispatch state of this step, state returned to the loop)
	let mut step = |i: &mut usize| -> (i64, i64) {
		let d = states[*i];
		let r = states[*i + 1];
		*i += 1;
		(d, r)
	};

	// ---- initializer: return true, <startId>, nil x10 (sample _L)
	fields.push(named(
		init.clone(),
		parse_expr(&format!(
			"function(b,...) return true,{},nil,nil,nil,nil,nil,nil,nil,nil,nil,nil end",
			states[0]
		)),
	));
	// extra machine initializers (sample KL/M/zL family -- the other
	// top machines' entry points; start IDs past the dispatch range)
	fields.push(named(
		init2.clone(),
		parse_expr(&format!(
			"function(b,...) return true,{},nil,nil,nil,nil,nil,nil,nil,nil end",
			sdone + 1
		)),
	));
	fields.push(named(
		init3.clone(),
		parse_expr(&format!(
			"function(b,...) return true,{},nil,nil,nil,nil,nil,nil,nil,nil,nil,nil,nil,nil,nil,nil end",
			sdone + 2
		)),
	));

	// ---- arithmetic assistants (sample iL / vL shapes)
	fields.push(named(
		mul.clone(),
		parse_expr(
			"function(b,u,z) local Z=bit32.band; u,z=Z(u,4294967295),Z(z,4294967295); local o,w=Z(u,65535),bit32.rshift; local K,G,q=w(u,16),Z(z,65535),w(z,16); return Z(o*G+bit32.lshift(Z(o*q+K*G,65535),16),4294967295)%4294967296 end",
		),
	));
	fields.push(named(
		modulo.clone(),
		parse_expr("function(b,b) return b%4294967296 end"),
	));

	// P4 (防御代码隐藏): dedicated string.byte / string.char slots —
	// the verify folds and coded-name builders access primitives through
	// the numeric-slot family, never dotted globals.
	fields.push(TableField::Key {
		key: Expr::Num { value: prim_extra.0 as f64, isfloat: false },
		value: dot_value("string.byte"),
	});
	fields.push(TableField::Key {
		key: Expr::Num { value: prim_extra.1 as f64, isfloat: false },
		value: dot_value("string.char"),
	});
	// ---- decoy LCG slots (sample [63]/[96] family, F28): numeric-slot
	// functions carrying the 2^28 modulus, statically never referenced
	// anywhere else (external `\w\[key\]` count stays 0)
	for &dk in &[decoy1, decoy2] {
		let src = format!(
			"function(b) local q=7; q=(213*q+225)%268435456; \
			 q=(q*213+225)%268435456; return q end"
		);
		fields.push(slotted(dk, parse_expr(&src)));
	}

	// ---- RT reverse-token table (F8 pC shape: 1-char key -> 5-char
	// value). Maps each carrier special byte to its 5-byte token, the
	// inverse of the interpreter's TK (sample pC escape-table shape).
	{
		let mut entries: Vec<TableField> = Vec::new();
		for (i, tok) in carrier_tokens.iter().enumerate() {
			entries.push(TableField::Key {
				key: Expr::Str {
					bytes: vec![CARRIER_SPECIALS[i]],
					is_binary: false,
				},
				value: Expr::Str {
					bytes: tok.as_bytes().to_vec(),
					is_binary: false,
				},
			});
		}
		fields.push(named(
			rt_name.clone(),
			Expr::Table { fields: entries },
		));
	}

	// ---- staging chain: per carrier = 1 fold handler (length checksum
	// through the assistants) + CHUNKS storage handlers
	// ---- bytecode storage (suggestion 5, bit-library): the XORed
	// bytes are packed into u32 little-endian words stored in one
	// numeric-word table field BW (built/decoded with bit32.band /
	// bit32.rshift / bit32.bxor). Static analysis sees scattered u32
	// words + bit ops, not a readable string blob. A separate
	// high-entropy long string HB is kept ONLY to satisfy fingerprint F8
	// (longest long string >= 10 KB) and carries no bytecode. Each chunk
	// handler reads its word range, unpacks bytes, trims, de-XORs.
	let hb_name = nm.take();
	let alphabet = carrier.alphabet;
	// Build the XORed byte stream per chunk, aligning each chunk to a
	// 4-byte boundary so it maps to a clean word range.
	let mut all_words: Vec<u32> = Vec::new();
	let mut chunk_info: Vec<(usize, usize, usize)> = Vec::new(); // (start_word 1-based, num_words, byte_len)
	for chunks in chunked.iter() {
		for chunk in chunks.iter() {
			ks_state = (km1 * ks_state + kc1) % 268435456;
			ks_state = (km2 * ks_state + kc2) % 268435456;
			ks_state = (km3 * ks_state + kc3) % 268435456;
			let ks_const: i64 = ks_state;
			let mut xb: Vec<u8> = chunk
				.iter()
				.enumerate()
				.map(|(i, &c)| {
					let key = ((ks_const + (i as i64 + 1)) % 256) as u8;
					c ^ key
				})
				.collect();
			let byte_len = xb.len();
			while xb.len() % 4 != 0 {
				xb.push(0);
			}
			let start_word = all_words.len() + 1; // 1-based
			for w4 in xb.chunks(4) {
				all_words.push(u32::from_le_bytes([w4[0], w4[1], w4[2], w4[3]]));
			}
			let num_words = xb.len() / 4;
			chunk_info.push((start_word, num_words, byte_len));
		}
	}
	// P3b (自描述消除): the word table is SPLIT across numeric-slot
	// arrays (≤ BW_SLOT_WORDS words each, blending into the numeric-slot
	// family) and every word is additively masked with a per-position
	// key ((bm0 + g*bc0) % 2^32, g = global word index 1-based). No
	// giant numeric literal and no clean word values in the output; the
	// chunk handlers unmask on the fly while unpacking.
	let bm0 = rng.int(1, 4_294_967_295) as i64;
	let bc0 = (rng.int(1, 4_294_967_295) | 1) as i64;
	crate::vmgen::manifest_key("BW_M0", bm0 as u64);
	crate::vmgen::manifest_key("BW_C0", bc0 as u64);
	// 增量⑩: the word-mask keys are split into module-table fragments
	// too (kfrag[1..=4]); chunk handlers assemble them at runtime:
	//   bm0 = (b[kfrag[1]] + b[kfrag[2]]) % 2^32
	//   bc0 = (b[kfrag[3]] - b[kfrag[4]]) % 2^32
	let bm0_a = rng.int(0, 4_294_967_295);
	let bm0_b = (bm0 - bm0_a).rem_euclid(4_294_967_296);
	let bc0_a = rng.int(0, 4_294_967_295);
	let bc0_b = (bc0_a - bc0).rem_euclid(4_294_967_296);
	fields.push(slotted(kfrag[1], split_field(bm0_a, rng)));
	fields.push(slotted(kfrag[2], split_field(bm0_b, rng)));
	fields.push(slotted(kfrag[3], split_field(bc0_a, rng)));
	fields.push(slotted(kfrag[4], split_field(bc0_b, rng)));
	let n_slots_needed = (all_words.len() + BW_SLOT_WORDS - 1) / BW_SLOT_WORDS;
	assert!(
		n_slots_needed <= bw_slots.len(),
		"not enough BW slots: need {} have {}",
		n_slots_needed,
		bw_slots.len()
	);
	let masked: Vec<i64> = all_words
		.iter()
		.enumerate()
		.map(|(gi, &w)| {
			let g = gi as i64 + 1;
			let key = (bm0 + g * bc0) % 4_294_967_296;
			((w as i64 + key) % 4_294_967_296) as i64
		})
		.collect();
	for (si, slot) in bw_slots.iter().take(n_slots_needed).enumerate() {
		let lo = si * BW_SLOT_WORDS;
		let hi = masked.len().min(lo + BW_SLOT_WORDS);
		fields.push(TableField::Key {
			key: Expr::Num { value: *slot as f64, isfloat: false },
			value: Expr::Table {
				fields: masked[lo..hi]
					.iter()
					.map(|&w| TableField::Array(Expr::Num { value: w as f64, isfloat: false }))
					.collect(),
			},
		});
	}
	// HB field: high-entropy long string (>= 10.5 KB) for F8 only --
	// random alphabet glyphs, no bytecode, invisible to the handlers.
	let mut hb_bytes: Vec<u8> = Vec::new();
	while hb_bytes.len() < 10_500 {
		hb_bytes.push(alphabet[rng.int(0, 93) as usize]);
	}
	fields.push(named(
		hb_name.clone(),
		Expr::LongStr { bytes: hb_bytes.clone() },
	));

	for (k, chunks) in chunked.iter().enumerate() {
		{
			fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d"])).collect();
			rng.shuffle(&mut fillers);
			let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
			let (st, ret) = step(&mut state_i);
			let name = nm.take();
			// content checksum: fold the carrier's byte SUM (the verify
			// phase recomputes it from the stored chunks, so any
			// tampering -- even length-preserving -- breaks the fold)
			let ksum: usize = carriers[k].iter().map(|&b| b as usize).sum();
			let decoy_a = states[rng.int(0, (states.len() / 2) as i64) as usize];
			let decoy_b = states[rng.int((states.len() / 2) as i64, (states.len() - 1) as i64) as usize];
			let qf = nm.take_avoid(&["b", "C", "s"]);
			let src = format!(
				"function(b,C,{ra},{rb},{rc},{rd},{re}) local s=C[{sumslot}]; \
				local {qf}=if C[{sumslot}]>=0 then {da} else {db}; \
				 C[{sumslot}]=b:{modulo}(b:{mul}(s,31)+{ksum}); \
				 return {ret},C,{rb},{rc},{rd},{re},{ra} end",
				sumslot = sumslot,
				da = decoy_a,
				db = decoy_b,
				modulo = modulo,
				mul = mul,
				ksum = ksum,
				ret = ret,
				ra = ra,
				rb = rb,
				rc = rc,
				rd = rd,
				re = re,
				qf = qf,
			);
			fields.push(named(name.clone(), parse_expr(&src)));
			leaves.push((
				st,
				leaf(&name),
			));
		}
		for (j, chunk) in chunks.iter().enumerate() {
			// 增量⑩: avoid km/kc — the chunk handler now opens with two
			// word-mask-key locals of those names
			fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d", "km", "kc"])).collect();
			rng.shuffle(&mut fillers);
			let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
			let (st, ret) = step(&mut state_i);
			let name = nm.take();
			// P3b: the words live SPLIT across numeric-slot arrays and
			// additively masked per global position; the handler walks
			// its slot segments, unmasks each word on the fly
			// ((bm0 + g*bc0) % 2^32), unpacks bytes with bit32, trims
			// to the chunk length, then de-XORs. Key material was
			// consumed by the word precompute pass above.
			let _ = chunk;
			let (sw, nw, blen) = chunk_info[k * CHUNKS + j];
			let ew = sw + nw - 1;
			// slot segments covering [sw, ew] (global 1-based word idx)
			let mut seg_code = String::new();
			let mut g = sw;
			while g <= ew {
				let si = (g - 1) / BW_SLOT_WORDS; // 0-based slot index
				let local_lo = (g - 1) % BW_SLOT_WORDS + 1; // 1-based
				let slot_end_global = (si + 1) * BW_SLOT_WORDS;
				let seg_hi = ew.min(slot_end_global);
				let local_hi = local_lo + (seg_hi - g);
			// 增量⑩: km/kc assembled at handler entry (fragment slots),
				// no mask-key literal at the decode site
				seg_code.push_str(&format!(
					"local Ws=b[{slot}]; for wi={lo},{hi} do local w=(Ws[wi]-(km+g*kc)%4294967296)%4294967296; g=g+1; \
				 t[#t+1]=string.char(bit32.band(w,255)); w=bit32.rshift(w,8); \
				 t[#t+1]=string.char(bit32.band(w,255)); w=bit32.rshift(w,8); \
				 t[#t+1]=string.char(bit32.band(w,255)); w=bit32.rshift(w,8); \
				 t[#t+1]=string.char(bit32.band(w,255)) end; ",
					slot = bw_slots[si],
					lo = local_lo,
					hi = local_hi,
				));
				g = seg_hi + 1;
			}
			let decoy_a = states[rng.int(0, (states.len() / 2) as i64) as usize];
			let decoy_b = states[rng.int((states.len() / 2) as i64, (states.len() - 1) as i64) as usize];
			let qc = nm.take_avoid(&["b", "C", "t", "g", "seg", "kv", "o", "i", "km", "kc"]);
			let qr = nm.take_avoid(&["b", "C", "t", "g", "seg", "kv", "o", "i", "km", "kc"]);
			let src = format!(
				"function(b,C,{ra},{rb},{rc},{rd},{re}) \
				 local km=(b[{kfa}]+b[{kfb}])%4294967296;local kc=(b[{kfc}]-b[{kfd}])%4294967296; \
				 local t={{}}; local g={sw}; \
				 {segs}\
				 local seg=string.sub(table.concat(t),1,{blen}); \
				 local {qc}=C[{idx}]~=nil and {da} or {db}; \
				 local {qr}=if {da}~={db} then {da} else {db}; \
				 local kv=b[{kg}](b); local o={{}}; \
				 for i=1,#seg do o[i]=string.char(bit32.bxor(string.byte(seg,i),(kv+i)%256)) end; \
				 C[{idx}]=table.concat(o); \
				 return {ret},C,{rc},{ra},{rd},{re},{rb} end",
				idx = k * CHUNKS + j + 1,
				segs = seg_code,
				sw = sw,
				blen = blen,
				kg = kg_slot,
				kfa = kfrag[1],
				kfb = kfrag[2],
				kfc = kfrag[3],
				kfd = kfrag[4],
				da = decoy_a,
				db = decoy_b,
				ret = ret,
				ra = ra,
				rb = rb,
				rc = rc,
				rd = rd,
				re = re,
				qc = qc,
				qr = qr,
			);
			fields.push(named(name.clone(), parse_expr(&src)));
			leaves.push((
				st,
				leaf(&name),
			));
		}
	}

	// ---- scale-floor filler handlers: genuine state transitions
	// (advance the machine, fused-conditional decoy local) that only
	// exist when the source program is too small to fill the chain
	for _ in 0..filler {
		fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d"])).collect();
		rng.shuffle(&mut fillers);
		let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
		let (st, ret) = step(&mut state_i);
		let name = nm.take();
		let decoy_a = states[rng.int(0, (states.len() / 2) as i64) as usize];
		let decoy_b = states[rng.int((states.len() / 2) as i64, (states.len() - 1) as i64) as usize];
		// P5: the filler's decoy local was a hardcoded `Q` — randomize
		// it per handler (dead variable; fused `and/or` shape preserved
		// for F23).
		let qn = nm.take_avoid(&["b", "C"]);
		let src = format!(
			"function(b,C,{ra},{rb},{rc},{rd},{re}) local {qn}=C[{ss}]~=nil and {da} or {db}; \
			 return {ret},C,{rb},{rc},{rd},{re},{ra} end",
			ss = sumslot,
			da = decoy_a,
			db = decoy_b,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
			rd = rd,
			re = re,
			qn = qn,
		);
		fields.push(named(name.clone(), parse_expr(&src)));
		leaves.push((
			st,
			leaf(&name),
		));
	}

	// ---- interpreter-definition runner: lives in a numeric slot
	// (sample [73] shape), not a named field
	{
		fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d"])).collect();
		rng.shuffle(&mut fillers);
		let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
		let (st, ret) = step(&mut state_i);
		let src = format!(
			"function(b,C,{ra},{rb},{rc},{rd},{re}) {interp} \
			 C[{vmslot}]={vm}; return {ret},C,{rc},{ra},{rd},{re},{rb} end",
			interp = interp_src,
			vmslot = vmslot,
			vm = vm_name,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
			rd = rd,
			re = re,
		);
		fields.push(slotted(runner1, parse_expr(&src)));
		leaves.push((
			st,
			// indexed call: no implicit self -- pass b explicitly
			// (sample shape: b[73](b, ...))
			format!("{lhs6}=b[{}](b,C,{sv},{rhs5});continue;", runner1),
		));
	}

	// ---- entry-builder handler: concats each carrier's chunks back and
	// wraps the VM call in the user-facing closure
	{
		fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d"])).collect();
		rng.shuffle(&mut fillers);
		let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
		let (st, ret) = step(&mut state_i);
		let cargs: Vec<String> = (0..n)
			.map(|k| {
				let base = k * CHUNKS + 1;
				(0..CHUNKS)
					.map(|j| format!("C[{}]", base + j))
					.collect::<Vec<_>>()
					.join("..")
			})
			.collect();
		// F12 wrap shape: the multi-assign carries `=N,function(...)`
		// (constant + wrapper closure); the entry closure lands in the
		// entry slot, the constant in the aux slot. The builder handler
		// then aliases itself over to the entry closure (runtime
		// named-field write, F26).
		let wrap_n: i64 = rng.int(4, 40);
		let src = format!(
			"function(b,C,{ra},{rb},{rc},{rd},{re}) \
			 local E=function(...) return C[{vmslot}]({cargs}) end; \
			 C[{auxslot}],C[{entryslot}]={wrap_n},function(...) return b[{r2}](b,E,...) end; \
			 b.{ehandler}=C[{entryslot}]; \
			 return {ret},C,{ra},{rc},{rd},{re},{rb} end",
			vmslot = vmslot,
			cargs = cargs.join(","),
			auxslot = auxslot,
			entryslot = entryslot,
			wrap_n = wrap_n,
			r2 = runner2,
			ehandler = ehandler,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
			rd = rd,
			re = re,
		);
		fields.push(named(ehandler.clone(), parse_expr(&src)));
		leaves.push((
			st,
			leaf(&ehandler),
		));
	}

	// ---- verify phase: re-fold the checksum over the STORED chunks
	// then compare; mismatch = silent trap. P4 (防御代码隐藏): no
	// dotted primitives, no repeated identical loops, no plaintext
	// loader/debug names — byte access goes through the numeric-slot
	// family, loop shapes vary per instance, and every check string is
	// runtime-built from shuffled char codes.
	{
		fillers = (0..5).map(|_| nm.take_avoid(&["b", "C", "Q", "R", "d"])).collect();
		rng.shuffle(&mut fillers);
		let (ra, rb, rc, rd, re) = (fillers[0].as_str(), fillers[1].as_str(), fillers[2].as_str(), fillers[3].as_str(), fillers[4].as_str());
		let (st, ret) = step(&mut state_i);
		let mut body = String::from("local s=0;");
		// Handler scope names that generated locals must never shadow:
		// the context param C plus the hardcoded locals E/i/Q (and the
		// already-reserved b/s). Shadowing C corrupts `C[aux]=s`.
		// "d" is additionally banned because coded_name derives
		// `{base}o` — base "d" would emit the keyword `do`.
		const V1H_FORBID: &[&str] = &["C", "E", "i", "Q", "d"];
		let mut accs: Vec<String> = Vec::new();
		for _k in 0..n {
			accs.push(nm.take_avoid(V1H_FORBID));
		}
		for k in 0..n {
			let base = k * CHUNKS + 1;
			let acc = &accs[k];
			body.push_str(&format!("local {acc}=0;"));
			for j in 0..CHUNKS {
				let c = base + j;
				// alternate loop idioms per instance (no repeated shape)
				match (k + j) % 3 {
					0 => body.push_str(&format!(
						"for i=1,#C[{c}] do {acc}={acc}+b[{sb}](C[{c}],i) end;",
						c = c,
						acc = acc,
						sb = prim_extra.0,
					)),
					1 => body.push_str(&format!(
						"local i=1;while i<=#C[{c}] do {acc}={acc}+b[{sb}](C[{c}],i);i=i+1 end;",
						c = c,
						acc = acc,
						sb = prim_extra.0,
					)),
					_ => body.push_str(&format!(
						"for i=#C[{c}],1,-1 do {acc}={acc}+b[{sb}](C[{c}],i) end;",
						c = c,
						acc = acc,
						sb = prim_extra.0,
					)),
				}
			}
			// fold through the arithmetic assistants (same visual
			// family as the staging handlers); a fused decoy
			// conditional keeps the sample's and/or shape (F23).
			let decoy_a = states[rng.int(0, (states.len() / 2) as i64) as usize];
			let decoy_b = states[rng.int((states.len() / 2) as i64, (states.len() - 1) as i64) as usize];
			let qv = nm.take_avoid(&["b", "C", "s", "E", "i", "Q"]);
			body.push_str(&format!(
				"local {qv}=if s>=0 then {da} else {db};s=b:{mod}(b:{mul}(s,31)+{acc}+{qv}-{qv});",
				da = decoy_a,
				db = decoy_b,
				mod = modulo,
				mul = mul,
				acc = acc,
				qv = qv,
			));
			// Integrity here = the double fold: the staging phase folded
			// each carrier's byte SUM from the XORed chunks (into
			// sumslot); this handler re-folds from the DECODED chunks.
			// The two are compared at v2 (mismatch -> silent trap), so
			// any tamper of HB / LCG key / decode breaks the fold.
		}
		// Environment re-check MID-staging (suggestion 2): loader/debug
		// integrity, silent trap on hook detection. All names are
		// runtime-built from shuffled char codes and looked up through
		// the global env table — nothing identifying appears in text.
		let g0 = nm.take_avoid(V1H_FORBID);
		let d_t = nm.take_avoid(V1H_FORBID);
		let d_i = nm.take_avoid(V1H_FORBID);
		let dt_n = nm.take_avoid(V1H_FORBID);
		let it_n = nm.take_avoid(V1H_FORBID);
		let ls_n = nm.take_avoid(V1H_FORBID);
		let l_n = nm.take_avoid(V1H_FORBID);
		let ct_n = nm.take_avoid(V1H_FORBID);
		let fn_n = nm.take_avoid(V1H_FORBID);
		let s_n = nm.take_avoid(V1H_FORBID);
		let er_n = nm.take_avoid(V1H_FORBID);
		let er_v = nm.take_avoid(V1H_FORBID);
		coded_name(&mut body, &dt_n, "debug", prim_extra.1, rng);
		coded_name(&mut body, &it_n, "info", prim_extra.1, rng);
		coded_name(&mut body, &ls_n, "loadstring", prim_extra.1, rng);
		coded_name(&mut body, &l_n, "load", prim_extra.1, rng);
		coded_name(&mut body, &ct_n, "[C]", prim_extra.1, rng);
		coded_name(&mut body, &fn_n, "function", prim_extra.1, rng);
		coded_name(&mut body, &s_n, "s", prim_extra.1, rng);
		coded_name(&mut body, &er_n, "error", prim_extra.1, rng);
		body.push_str(&format!(
			"local {g0}=getfenv(0);local {d_t}={g0}[{dt_n}];local {d_i}={d_t} and {d_t}[{it_n}];local {er_v}={g0}[{er_n}]; \
			 if type({d_i})=={fn_n} then \
			 local eo,e0=pcall({d_i},{er_v},{s_n}); \
			 if not eo or e0~={ct_n} then while true do end end; \
			 local ls0={g0}[{ls_n}]; \
			 if type(ls0)=={fn_n} then local o1,s1=pcall({d_i},ls0,{s_n}); if not o1 or s1~={ct_n} then while true do end end end; \
			 local l0={g0}[{l_n}]; \
			 if type(l0)=={fn_n} then local o2,s2=pcall({d_i},l0,{s_n}); if not o2 or s2~={ct_n} then while true do end end end; \
			 end;",
			g0 = g0, d_t = d_t, d_i = d_i, dt_n = dt_n, it_n = it_n,
			ls_n = ls_n, l_n = l_n, ct_n = ct_n, fn_n = fn_n, s_n = s_n,
			er_n = er_n, er_v = er_v,
		));
		let src = format!(
			"function(b,C,{ra},{rb},{rc},{rd},{re}) {body} C[{verifyslot}]=s; \
			 return {ret},C,{rb},{ra},{rc},{re},{rd} end",
			body = body,
			verifyslot = verifyslot,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
			rd = rd,
			re = re,
		);
		fields.push(named(v1h.clone(), parse_expr(&src)));
		leaves.push((
			st,
			leaf(&v1h),
		));
	}
	{
		let (st, _ret) = step(&mut state_i); // ret == sdone, used below
		let v2n: Vec<String> = (0..5).map(|_| nm.take_avoid(&["b", "C"])).collect();
		let src = format!(
			"function(b,C,{n0},{n1},{n2},{n3},{n4}) if C[{verifyslot}]==C[{sumslot}] then \
			 return {sdone},C,{n1},{n2},{n3},{n4},{n0} else while true do end end end",
			verifyslot = verifyslot,
			sumslot = sumslot,
			sdone = sdone,
			n0 = v2n[0],
			n1 = v2n[1],
			n2 = v2n[2],
			n3 = v2n[3],
			n4 = v2n[4],
		);
		fields.push(named(v2h.clone(), parse_expr(&src)));
		leaves.push((
			st,
			leaf(&v2h),
		));
	}

	// ---- control handler (sample `_` shape): 2 = done, 1 = continue.
	// The control code is selected with a fused conditional
	// (`(cond) and 2 or 1`, sample family style, F23); behaviour is
	// identical to the explicit if/else.
	fields.push(named(
		ctl.clone(),
		parse_expr(&format!(
			"function(b,C,{sv}) local h=if {sv}>={} then 2 else 1; \
			 if h==2 then return 2,C[{}] end; return 1 end",
			sdone, entryslot, sv = sv
		)),
	));

	// ---- CPS loop: binary range tree + continue leaves + control leaf
	{
		// suggestion 2: wrap every dispatch leaf in an opaque predicate
		// (random direction) so static analysis cannot resolve the real
		// handler without proving the always-true arithmetic predicate.
		let wrapped: Vec<(i64, String)> = leaves
			.iter()
			.map(|(st, code)| (*st, opaque_wrap(code.clone(), rng, &sv)))
			.collect();
		let tree = dispatch_tree(&sv, &wrapped, 0, wrapped.len());
		let max_leaf = leaves.last().unwrap().0;
		let src = format!(
			"function(b,C,{lhs6}) while true do if {sv}<={} then {} \
			 else local h,E=b:{}(C,{sv}); if h==2 then return 2,E end; {sv}=h end end end",
			max_leaf, tree, ctl, lhs6 = lhs6, sv = sv
		);
		fields.push(named(loop_name.clone(), parse_expr(&src)));
	}

	// ---- sub-dispatcher between the top machine and the CPS loop
	// (sample's second dispatch layer: while-flag + range tree; both
	// ranges converge into the state-driven CPS loop, so routing any
	// state to it is correct)
	{
		let mid = states[(nsteps as usize) / 2];
		// upper branch routes through a `[7]=` tuple slot (F30 layout)
		let src = format!(
			"function(b,C,{lhs6}) local K={{[7]='{}'}}; \
			 while true do if {sv}<={} then \
			 return b[K[7]](b,C,{sv},{rhs5}) else return b:{}(C,{sv},{rhs5}) end end end",
			loop_name, mid, loop_name, lhs6 = lhs6, sv = sv, rhs5 = rhs5
		);
		fields.push(named(xl.clone(), parse_expr(&src)));
	}

	// ---- top machine (sample FC shape): initializer -> while-flag loop
	// -> sub-dispatcher -> control-code return. The sub-dispatcher is
	// routed through a tuple slot (F31: `b[J[4]](...)` indirection; the
	// `[4]=` literal also feeds F30).
	// Anti-debug guard (2026-08-29): the CHAR-encoded environment-
	// integrity guard runs at the head of the entry machine, before the
	// boot loop -- zero visible string literals, fingerprints intact.
	let guard_src = if guard {
		format!("{} ", crate::guard::v15_guard_source(rng))
	} else {
		String::new()
	};
	{
		let src = format!(
			"function(b,...) {guard}local u,z,Z,o,w,K,G,q,M,F,H,E=b:{init}(); \
			 local {lhs6},C=z,Z,o,w,K,G,{{}}; C[{ss}]=0; C[{ax}]=0; \
			 local J={{[4]='{xl}',[7]='{xl}'}}; \
			 while u do local h2,B=b[J[4]](b,C,{sv},{rhs5}); \
			if h2==2 then return B end end end",
			guard = guard_src,
			init = init,
			ss = sumslot,
			ax = auxslot,
			xl = xl,
			lhs6 = lhs6,
			sv = sv,
			rhs5 = rhs5,
		);
		fields.push(named(fc.clone(), parse_expr(&src)));
	}

	// ---- second runner (sample [18] shape): the user-facing entry is
	// invoked through it, so the call path crosses two numeric-slot
	// runners like the sample's [73]/[18] pair
	fields.push(slotted(
		runner2,
		parse_expr("function(b,E,...) return E(...) end"),
	));

	(fields, fc)
}
