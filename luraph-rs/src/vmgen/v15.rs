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
pub fn module_fields(rng: &mut Rng) -> Vec<TableField> {
	let mut slots: Vec<i64> = (1..=126).collect();
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
) -> (Vec<(String, Expr)>, String) {
	let mut nm = Names::new(rng);
	let fc = nm.take(); // entry machine (xC family)
	let init = nm.take(); // initializer (xL family)
	let mul = nm.take(); // mul32 assistant (xL)
	let modulo = nm.take(); // mod 2^32 assistant (xL)
	let loop_name = nm.take(); // CPS loop (single)
	let ctl = nm.take(); // control handler (single)
	let dhandler = nm.take(); // interpreter-definition handler
	let mut ghandlers: Vec<String> = carriers
		.iter()
		.map(|_| nm.take())
		.collect();

	// state pool: s0 (first staging) .. sdef .. sdone, ascending
	let n = carriers.len();
	let need = (n + 3) as i64;
	let mut ids: Vec<i64> = (3..=236).collect();
	rng.shuffle(&mut ids);
	let mut states: Vec<i64> = ids[..need as usize].to_vec();
	states.sort();
	let s0 = states[0];
	let sdef = states[n];
	let sdone = states[n + 1];

	// context-table layout: C[1..n]=carriers, C[n+1]=entry, C[n+2]=checksum
	let eidx = n + 1;
	let sumidx = n + 2;

	let mut fields: Vec<(String, Expr)> = Vec::new();

	// ---- initializer: return true, <startId>, nil×10 (sample _L shape)
	fields.push((
		init.clone(),
		parse_expr(&format!(
			"function(b,...) return true,{},nil,nil,nil,nil,nil,nil,nil,nil,nil,nil end",
			s0
		)),
	));

	// ---- arithmetic assistants (sample iL / vL shapes, real callers:
	// the staging handlers accumulate a checksum through them)
	fields.push((
		mul.clone(),
		parse_expr(
			"function(b,u,z) local Z=bit32.band; u,z=Z(u,4294967295),Z(z,4294967295); local o,w=Z(u,65535),bit32.rshift; local K,G,q=w(u,16),Z(z,65535),w(z,16); return Z(o*G+bit32.lshift(Z(o*q+K*G,65535),16),4294967295)%4294967296 end",
		),
	));
	fields.push((
		modulo.clone(),
		parse_expr("function(b,b) return b%4294967296 end"),
	));

	// ---- carrier staging handlers: one per prototype. Each stores its
	// carrier into the context table and folds its length into the
	// checksum through the assistants (genuine calls). Tuple fillers are
	// permuted per handler (sample style).
	let mut fillers = vec!["p1", "p2", "p3"];
	for (k, carrier) in carriers.iter().enumerate() {
		rng.shuffle(&mut fillers);
		let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
		let next = if k + 1 < n { states[k + 1] } else { sdef };
		let src = format!(
			"function(b,C,{ra},{rb},{rc}) local s=C[{sumidx}]; \
			 C[{sumidx}]=b:{modulo}(b:{mul}(s,31)+{len}); \
			 C[{ck}]={lit}; return {next},C,{rb},{rc},{ra} end",
			sumidx = sumidx,
			modulo = modulo,
			mul = mul,
			len = carrier.len(),
			ck = k + 1,
			lit = lua_bytes_lit(carrier),
			next = next,
			ra = ra,
			rb = rb,
			rc = rc,
		);
		fields.push((ghandlers[k].clone(), parse_expr(&src)));
	}

	// ---- interpreter-definition handler: embeds the passed interpreter
	// (defines the VM local), builds the entry closure over the staged
	// carriers, stores it in the context table.
	{
		rng.shuffle(&mut fillers);
		let (ra, rb, rc) = (fillers[0], fillers[1], fillers[2]);
		let crefs: Vec<String> = (1..=n).map(|i| format!("C[{}]", i)).collect();
		let src = format!(
			"function(b,C,{ra},{rb},{rc}) {interp} \
			 local E=function(...) return {vm}({crefs}) end; \
			 C[{eidx}]=E; return {sdone},C,{rc},{ra},{rb} end",
			interp = interp_src,
			vm = vm_name,
			crefs = crefs.join(","),
			eidx = eidx,
			sdone = sdone,
			ra = ra,
			rb = rb,
			rc = rc,
		);
		fields.push((dhandler.clone(), parse_expr(&src)));
	}

	// ---- control handler (sample `_` shape): 2 = done, 1 = continue
	fields.push((
		ctl.clone(),
		parse_expr(&format!(
			"function(b,C,V) if V>={} then return 2,C[{}] else return 1 end end",
			sdone, eidx
		)),
	));

	// ---- CPS loop: binary range tree, leaves = handler call + continue;
	// the >sdef range is the control leaf (sample `a` shape).
	{
		let mut leaves: Vec<(i64, String)> = Vec::new();
		for (k, st) in states[..n].iter().enumerate() {
			leaves.push((
				*st,
				format!(
					"V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;",
					ghandlers[k]
				),
			));
		}
		leaves.push((
			sdef,
			format!("V,C,f1,f2,f3=b:{}(C,f1,f2,f3);continue;", dhandler),
		));
		let tree = dispatch_tree("V", &leaves, 0, leaves.len());
		let src = format!(
			"function(b,C,V,f1,f2,f3) while true do if V<={} then {} \
			 else local h,E=b:{}(C,V); if h==2 then return 2,E end; V=h end end end",
			sdef, tree, ctl
		);
		fields.push((loop_name.clone(), parse_expr(&src)))
	}

	// ---- top machine (sample FC shape): initializer + tuple rename +
	// while-flag loop driving the CPS loop; control code 2 = return the
	// entry closure.
	{
		let src = format!(
			"function(b,...) local u,z,Z,o,w,K,G,q,M,F,H,E=b:{}(); \
			 local V,f1,f2,f3,C=z,Z,o,w,{{}}; C[{}]=0; \
			 while u do local h2,B=b:{}(C,V,f1,f2,f3); \
			 if h2==2 then return B end end end",
			init, sumidx, loop_name
		);
		fields.push((fc.clone(), parse_expr(&src)));
	}

	let _ = ghandlers.remove(0); // silence unused warning when n==1 path varies
	(fields, fc)
}
