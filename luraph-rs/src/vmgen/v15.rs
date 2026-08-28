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
pub fn module_fields(rng: &mut Rng, exclude: &[i64]) -> Vec<TableField> {
	let mut slots: Vec<i64> = (1..=126)
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
		for b in singles {
			pool.push(b.to_string());
			pool.push(format!("{}L", b));
			pool.push(format!("{}C", b));
		}
		rng.shuffle(&mut pool);
		// P2 constant fields + self param are reserved
		let reserved = [
			"AL", "BL", "EL", "PL", "QL", "XC", "cL", "dL", "gL", "RC",
			"pC", "h", "s", "b",
		];
		pool.retain(|n| !reserved.contains(&n.as_str()));
		Names { pool }
	}
	fn take(&mut self) -> String {
		self.pool.pop().unwrap()
	}
}

/// Binary dispatch tree over sorted (state, leaf) pairs (sample style:
/// `if V<=N then ... else ...` range tree).
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
pub fn scaffold(
	rng: &mut Rng,
	interp_src: &str,
	vm_name: &str,
	carriers: &[Vec<u8>],
	runner1: i64,
	runner2: i64,
) -> (Vec<TableField>, String) {
	let mut nm = Names::new(rng);
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
	const CHUNKS: usize = 3;

	// P4 carrier keystream: each chunk's bytes are XORed at build time
	// with a positional key (K1*i + K2) % 256 and the chunk handlers
	// reverse it at runtime via bit32.bxor -- the stored literals are no
	// longer directly base-94 readable (fingerprint F16: bit32.bxor +
	// %256 both present in the decode path). K1/K2 are per-build random.
	let ks1: i64 = rng.int(1, 255);
	let ks2: i64 = rng.int(0, 255);

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

	// one state per step + terminal; ascending
	let nsteps = n * (1 + CHUNKS) + 4;
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
	let mut fillers = vec!["p1", "p2", "p3"];
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

	// ---- staging chain: per carrier = 1 fold handler (length checksum
	// through the assistants) + CHUNKS storage handlers
	for (k, chunks) in chunked.iter().enumerate() {
		{
			rng.shuffle(&mut fillers);
			let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
			let (st, ret) = step(&mut state_i);
			let name = nm.take();
			// content checksum: fold the carrier's byte SUM (the verify
			// phase recomputes it from the stored chunks, so any
			// tampering -- even length-preserving -- breaks the fold)
			let ksum: usize = carriers[k].iter().map(|&b| b as usize).sum();
			let src = format!(
				"function(b,C,{ra},{rb},{rc}) local s=C[{sumslot}]; \
				 C[{sumslot}]=b:{modulo}(b:{mul}(s,31)+{ksum}); \
				 return {ret},C,{rb},{rc},{ra} end",
				sumslot = sumslot,
				modulo = modulo,
				mul = mul,
				ksum = ksum,
				ret = ret,
				ra = ra,
				rb = rb,
				rc = rc,
			);
			fields.push(named(name.clone(), parse_expr(&src)));
			leaves.push((
				st,
				format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", name),
			));
		}
		for (j, chunk) in chunks.iter().enumerate() {
			rng.shuffle(&mut fillers);
			let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
			let (st, ret) = step(&mut state_i);
			let name = nm.take();
			// P4: XOR the chunk bytes with the positional keystream so the
			// stored literal is opaque; the handler reverses it at runtime.
			let xored: Vec<u8> = chunk
				.iter()
				.enumerate()
				.map(|(i, &c)| {
					let key = ((ks1 * (i as i64 + 1) + ks2) % 256) as u8;
					c ^ key
				})
				.collect();
			let src = format!(
				"function(b,C,{ra},{rb},{rc}) local seg={lit}; local t={{}}; \
				 for i=1,#seg do t[i]=string.char(bit32.bxor(string.byte(seg,i),({k1}*i+{k2})%256)) end; \
				 C[{idx}]=table.concat(t); \
				 return {ret},C,{rc},{ra},{rb} end",
				idx = k * CHUNKS + j + 1,
				lit = lua_bytes_lit(&xored),
				k1 = ks1,
				k2 = ks2,
				ret = ret,
				ra = ra,
				rb = rb,
				rc = rc,
			);
			fields.push(named(name.clone(), parse_expr(&src)));
			leaves.push((
				st,
				format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", name),
			));
		}
	}

	// ---- interpreter-definition runner: lives in a numeric slot
	// (sample [73] shape), not a named field
	{
		rng.shuffle(&mut fillers);
		let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
		let (st, ret) = step(&mut state_i);
		let src = format!(
			"function(b,C,{ra},{rb},{rc}) {interp} \
			 C[{vmslot}]={vm}; return {ret},C,{rc},{ra},{rb} end",
			interp = interp_src,
			vmslot = vmslot,
			vm = vm_name,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
		);
		fields.push(slotted(runner1, parse_expr(&src)));
		leaves.push((
			st,
			// indexed call: no implicit self -- pass b explicitly
			// (sample shape: b[73](b, ...))
			format!("V,C,f1,f2,f3=b[{}](b,C,f1,f2,f3);continue;", runner1),
		));
	}

	// ---- entry-builder handler: concats each carrier's chunks back and
	// wraps the VM call in the user-facing closure
	{
		rng.shuffle(&mut fillers);
		let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
		let (st, ret) = step(&mut state_i);
		let cargs: Vec<String> = (0..n)
			.map(|k| {
				let base = k * CHUNKS + 1;
				format!("C[{}]..C[{}]..C[{}]", base, base + 1, base + 2)
			})
			.collect();
		let src = format!(
			"function(b,C,{ra},{rb},{rc}) \
			 local E=function(...) return C[{vmslot}]({cargs}) end; \
			 C[{entryslot}]=function(...) return b[{r2}](b,E,...) end; \
			 return {ret},C,{ra},{rc},{rb} end",
			vmslot = vmslot,
			cargs = cargs.join(","),
			entryslot = entryslot,
			r2 = runner2,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
		);
		fields.push(named(ehandler.clone(), parse_expr(&src)));
		leaves.push((
			st,
			format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", ehandler),
		));
	}

	// ---- verify phase (sample site3 shape): re-fold the checksum over
	// the STORED chunks (real integrity check) then compare; mismatch =
	// silent trap (no os.clock -- fingerprint F18 safe)
	{
		rng.shuffle(&mut fillers);
		let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
		let (st, ret) = step(&mut state_i);
		let mut body = String::from("local s=0;");
		for k in 0..n {
			let base = k * CHUNKS + 1;
			// re-fold the byte sums from the STORED chunks
			body.push_str(&format!(
				"local t{k}=0;for i=1,#C[{a}] do t{k}=t{k}+string.byte(C[{a}],i) end;\
				 for i=1,#C[{b2}] do t{k}=t{k}+string.byte(C[{b2}],i) end;\
				 for i=1,#C[{c}] do t{k}=t{k}+string.byte(C[{c}],i) end;\
				 s=b:{mod}(b:{mul}(s,31)+t{k});",
				k = k,
				a = base,
				b2 = base + 1,
				c = base + 2,
				mod = modulo,
				mul = mul,
			));
		}
		let src = format!(
			"function(b,C,{ra},{rb},{rc}) {body} C[{verifyslot}]=s; \
			 return {ret},C,{rb},{ra},{rc} end",
			body = body,
			verifyslot = verifyslot,
			ret = ret,
			ra = ra,
			rb = rb,
			rc = rc,
		);
		fields.push(named(v1h.clone(), parse_expr(&src)));
		leaves.push((
			st,
			format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", v1h),
		));
	}
	{
		let (st, _ret) = step(&mut state_i); // ret == sdone, used below
		let src = format!(
			"function(b,C,p1,p2,p3) if C[{verifyslot}]==C[{sumslot}] then \
			 return {sdone},C,p2,p3,p1 else while true do end end end",
			verifyslot = verifyslot,
			sumslot = sumslot,
			sdone = sdone,
		);
		fields.push(named(v2h.clone(), parse_expr(&src)));
		leaves.push((
			st,
			format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", v2h),
		));
	}

	// ---- control handler (sample `_` shape): 2 = done, 1 = continue.
	// The control code is selected with a fused conditional
	// (`(cond) and 2 or 1`, sample family style, F23); behaviour is
	// identical to the explicit if/else.
	fields.push(named(
		ctl.clone(),
		parse_expr(&format!(
			"function(b,C,V) local h=(V>={}) and 2 or 1; \
			 if h==2 then return 2,C[{}] end; return 1 end",
			sdone, entryslot
		)),
	));

	// ---- CPS loop: binary range tree + continue leaves + control leaf
	{
		let tree = dispatch_tree("V", &leaves, 0, leaves.len());
		let max_leaf = leaves.last().unwrap().0;
		let src = format!(
			"function(b,C,V,f1,f2,f3) while true do if V<={} then {} \
			 else local h,E=b:{}(C,V); if h==2 then return 2,E end; V=h end end end",
			max_leaf, tree, ctl
		);
		fields.push(named(loop_name.clone(), parse_expr(&src)));
	}

	// ---- sub-dispatcher between the top machine and the CPS loop
	// (sample's second dispatch layer: while-flag + range tree; both
	// ranges converge into the state-driven CPS loop, so routing any
	// state to it is correct)
	{
		let mid = states[(nsteps as usize) / 2];
		let src = format!(
			"function(b,C,V,f1,f2,f3) while true do if V<={} then \
			 return b:{}(C,V,f1,f2,f3) else return b:{}(C,V,f1,f2,f3) end end end",
			mid, loop_name, loop_name
		);
		fields.push(named(xl.clone(), parse_expr(&src)));
	}

	// ---- top machine (sample FC shape): initializer -> while-flag loop
	// -> sub-dispatcher -> control-code return
	{
		let src = format!(
			"function(b,...) local u,z,Z,o,w,K,G,q,M,F,H,E=b:{}(); \
			 local V,f1,f2,f3,C=z,Z,o,w,{{}}; C[{}]=0; \
			 while u do local h2,B=b:{}(C,V,f1,f2,f3); \
			 if h2==2 then return B end end end",
			init, sumslot, xl
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
