//! luraph-rs — commercial-grade Lua 5.1 / Luau obfuscator.
//!
//! Pipeline: parse -> symtab -> [junk] -> [mangle] -> [flatten] -> [strings] -> print
//! (later: numbers/body/antidbg/vmgen)

mod anti;
mod antidbg;
mod guard;
mod ast;
mod body;
mod flatten;
mod junk;
mod lexer;
mod mangle;
mod minify;
mod numbers;
mod parser;
mod printer;
mod rng;
mod strings;
mod symtab;
mod vmgen;

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
	dialect: &'static str, // "5.1" | "luau"
	input: String,
	output: Option<String>,
	seed: u64,
	do_mangle: bool,
	do_minify: bool,
	do_numbers: bool,
	do_body: bool,
	do_antidbg: bool,
	/// Anti-debug / environment-integrity guard prelude (2026-08-29):
	/// self-contained IIFE injected ahead of the payload; aborts on
	/// hooked/stripped/debugged environments. Standard pipeline only —
	/// v15 keeps its 3-line sample form (a CHAR-encoded v15 variant is
	/// a future increment).
	do_guard: bool,
	do_vm: bool,
	/// v15 structural-parity profile (Route A, 2026-08-25): Luau/Roblox-only
	/// output shaped like a Luraph v15 sample (module table + FC entry).
	/// P0: flag exists and is Luau-gated; emission still routes through the
	/// legacy VM pipeline until the v15 emitter lands (P1+, see
	/// docs/v15-structural-parity-plan.md).
	do_v15: bool,
	do_strings: bool,
	do_flatten: bool,
	do_junk: bool,
	/// How many junk blocks to inject at each function head (max preset
	/// raises this; VM templates stay at 2 to keep the 200-local budget).
	junk_n: usize,
}

/// Named strength presets. Individual `--no-*` / `--vm` flags that
/// appear *after* `--preset` override the bundle (left-to-right).
fn apply_preset(opts: &mut Options, name: &str) -> Result<(), String> {
	opts.do_mangle = false;
	opts.do_minify = false;
	opts.do_strings = false;
	opts.do_flatten = false;
	opts.do_junk = false;
	opts.do_numbers = false;
	opts.do_body = false;
	opts.do_antidbg = false;
	opts.do_guard = false;
	opts.do_vm = false;
	opts.do_v15 = false;
	opts.junk_n = 2;
	match name {
		"low" => {
			// L1 + L2
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = true;
		}
		"medium" => {
			// low + L3
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = true;
			opts.do_flatten = true;
			opts.do_junk = true;
		}
		"high" => {
			// medium + L4 + L5 + L7  (default when no --preset)
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = true;
			opts.do_flatten = true;
			opts.do_junk = true;
			opts.do_numbers = true;
			opts.do_body = true;
			opts.do_antidbg = true;
			opts.do_guard = true;
		}
		"vm" => {
			// high + L6
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = true;
			opts.do_flatten = true;
			opts.do_junk = true;
			opts.do_numbers = true;
			opts.do_body = true;
			opts.do_antidbg = true;
			opts.do_guard = true;
			opts.do_vm = true;
		}
		"max" => {
			// strongest shipping preset = vm. v2 (CPS frames /
			// superinstructions) is reserved; extra junk would blow
			// the VM template's 200-local budget, so intensity stays 2.
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = true;
			opts.do_flatten = true;
			opts.do_junk = true;
			opts.do_numbers = true;
			opts.do_body = true;
			opts.do_antidbg = true;
			opts.do_guard = true;
			opts.do_vm = true;
			opts.junk_n = 2;
		}
		"v15" => {
			// Route A (2026-08-25): Luraph-v15 structural clone profile,
			// Luau/Roblox only. Emission = module table + :<boot>()(...)
			// shell (P1). L5 whole-program encryption and L7 time traps
			// stay OFF for this profile: the sample shape has neither
			// (fingerprints F2/F18), and loadstring/os.clock would mark
			// the output as not-v15-family. L2 string encryption is also
			// OFF at P1: chunk-splitting would balloon the literal count
			// (F10 wants the tens range; sample has 28). Interpreter
			// strings re-enter encrypted form with the P4 blob, which is
			// where the sample keeps all program strings.
			opts.do_mangle = true;
			opts.do_minify = true;
			opts.do_strings = false;
			opts.do_flatten = true;
			opts.do_junk = true;
			// numbers OFF: self-mod writes and dispatch constants must
			// stay raw literals (F5/F14 shape; splitting rewrote the
			// alias writes and state constants)
			opts.do_numbers = false;
			opts.do_body = false;
			opts.do_antidbg = false;
			// anti-debug guard: injected INSIDE the FC entry machine
			// (CHAR-encoded, zero visible strings; the standard prelude
			// form would break the 3-line sample shape)
			opts.do_guard = true;
			opts.do_vm = true;
			opts.do_v15 = true;
		}
		other => {
			return Err(format!(
				"unknown --preset '{other}' (want low|medium|high|vm|max|v15)"
			));
		}
	}
	Ok(())
}

fn print_help() {
	print!(
		"luraph-rs {} — commercial-grade Lua 5.1 / Luau obfuscator
Usage: luraph-rs [options] <input.lua> [output.lua]

Options:
  --dialect <5.1|luau>   target dialect (default: 5.1)
  -o, --output <file>    output file (default: stdout)
  --seed <n>             PRNG seed (default: time-based; use a fixed seed
                         for reproducible output)
  --preset <name>        named strength (flags after this override it):
                           low     L1+L2  name+minify+strings
                           medium  low+L3 flatten+junk
                           high    medium+L4+L5+L7  (default)
                           vm      high+L6 private bytecode VM
                           max     strongest shipping (= vm; v2 reserved)
                           v15     Luraph-v15 structural clone (Luau/Roblox
                                   only; Route A -- see docs/v15-
                                   structural-parity-plan.md)
  --minify               L1 minify output to a single compact line (default: enabled)
  --no-minify            keep the normalized (indented) printer output
  --no-numbers           disable L4 numeric literal rewriting (default: enabled)
  --no-body              disable L5 whole-program encryption (default: enabled)
  --no-antidbg           disable L7 anti-tamper (default: enabled)
  --no-guard             disable the anti-debug environment guard prelude
                         (default: enabled for the standard pipeline)
  --vm                   L6: compile the program to private bytecode and
                         run it through a generated obfuscated interpreter
  --no-mangle            disable L1 name mangling (default: enabled)
  --no-strings           disable L2 string encryption (default: enabled)
  --no-flatten           disable L3 loop desugar + CFG flattening (default: enabled)
  --no-junk              disable L3 junk code injection (default: enabled)
  -h, --help             show this help
  --version              show version
",
		VERSION
	);
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let mut opts = Options {
		dialect: "5.1",
		input: String::new(),
		output: None,
		seed: 0,
		do_mangle: true,
		do_minify: true,
		do_numbers: true,
		do_body: true,
		do_antidbg: true,
		do_guard: true,
		do_vm: false,
		do_v15: false,
		do_strings: true,
		do_flatten: true,
		do_junk: true,
		junk_n: 2,
	};
	let mut i = 0;
	let mut positional: Vec<String> = Vec::new();
	while i < args.len() {
		match args[i].as_str() {
			"-h" | "--help" => {
				print_help();
				return ExitCode::SUCCESS;
			}
			"--version" => {
				println!("luraph-rs {}", VERSION);
				return ExitCode::SUCCESS;
			}
			"--dialect" => {
				i += 1;
				if i >= args.len() {
					eprintln!("error: --dialect requires a value (5.1|luau)");
					return ExitCode::FAILURE;
				}
				match args[i].as_str() {
					"5.1" => opts.dialect = "5.1",
					"luau" => opts.dialect = "luau",
					other => {
						eprintln!("error: unknown dialect '{}'", other);
						return ExitCode::FAILURE;
					}
				}
			}
			"-o" | "--output" => {
				i += 1;
				if i >= args.len() {
					eprintln!("error: --output requires a file");
					return ExitCode::FAILURE;
				}
				opts.output = Some(args[i].clone());
			}
			"--seed" => {
				i += 1;
				if i >= args.len() {
					eprintln!("error: --seed requires a number");
					return ExitCode::FAILURE;
				}
				match args[i].parse() {
					Ok(v) => opts.seed = v,
					Err(_) => {
						eprintln!("error: --seed must be a number");
						return ExitCode::FAILURE;
					}
				}
			}
			"--preset" => {
				i += 1;
				if i >= args.len() {
					eprintln!("error: --preset requires a value (low|medium|high|vm|max|v15)");
					return ExitCode::FAILURE;
				}
				if let Err(e) = apply_preset(&mut opts, args[i].as_str()) {
					eprintln!("error: {e}");
					return ExitCode::FAILURE;
				}
			}
			"--no-mangle" => opts.do_mangle = false,
		"--minify" => opts.do_minify = true,
		"--no-minify" => opts.do_minify = false,
		"--no-numbers" => opts.do_numbers = false,
		"--no-body" => opts.do_body = false,
		"--no-antidbg" => opts.do_antidbg = false,
		"--no-guard" => opts.do_guard = false,
		"--vm" => opts.do_vm = true,
			"--no-strings" => opts.do_strings = false,
			"--no-flatten" => opts.do_flatten = false,
			"--no-junk" => opts.do_junk = false,
			s if s.starts_with('-') => {
				eprintln!("error: unknown option '{}'", s);
				print_help();
				return ExitCode::FAILURE;
			}
			_ => positional.push(args[i].clone()),
		}
		i += 1;
	}
	if positional.is_empty() {
		eprintln!("error: missing input file");
		print_help();
		return ExitCode::FAILURE;
	}
	opts.input = positional[0].clone();
	if positional.len() > 1 {
		opts.output = Some(positional[1].clone());
	}

	if opts.do_v15 && opts.dialect != "luau" {
		eprintln!(
			"error: --preset v15 requires --dialect luau\n  \
			 the v15 structural profile clones the Luraph v15 shape \
			 (buffer/bit32/typeof/setfenv),\n  which Lua 5.1 cannot \
			 run. Use `--preset vm` for the dual-target output."
		);
		return ExitCode::FAILURE;
	}

	let luau = opts.dialect == "luau";
	let seed = if opts.seed == 0 {
		// time-based default seed
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_nanos() as u64)
			.unwrap_or(1)
	} else {
		opts.seed
	};

	let src = match std::fs::read_to_string(&opts.input) {
		Ok(s) => s,
		Err(e) => {
			eprintln!("error: cannot read {}: {}", opts.input, e);
			return ExitCode::FAILURE;
		}
	};

	let result = (|| -> Result<String, String> {
		let mut block = parser::parse(&src, luau).map_err(|e| e.to_string())?;
		let mut table = symtab::resolve(&mut block);
		let mut rng = rng::Rng::new(seed);
		// L6: compile the program to private bytecode; the executable
		// program becomes the (to-be-obfuscated) interpreter template +
		// the bytecode passed as string literals to its entry call
		let vm_raw = std::env::var("LURAPH_VM_RAW").is_ok();
		let (mut block, mut table, v15_done) = if opts.do_vm {
			let program = vmgen::compile(&block, &table, &mut rng, !luau, opts.do_v15);
			let n = program.fns.len();
			let tsrc = vmgen::template::generate(
				&program.opmap,
				&program.slot_perm,
				&program.carrier,
				&mut rng,
				n,
				opts.do_v15,
				&program.nop_sites,
				&program.operand_sums,
				(program.ck_km, program.ck_kc),
				(program.mk1, program.mk2),
			);
			if std::env::var("LURAPH_VM_TSRC").is_ok() {
				std::fs::write("/tmp/vm_tsrc.lua", &tsrc).unwrap();
			}
			let mut tblock = parser::parse(&tsrc, luau).map_err(|e| e.to_string())?;
			let carrier_bytes: Vec<Vec<u8>> = program
				.fns
				.iter()
				.map(|b| program.carrier.encode(b))
				.collect();
			if opts.do_v15 {
				// P3 increment 1 (Route A): CPS bootstrap scaffold. The
				// interpreter block runs through the passes first (junk +
				// mangle; strings/numbers/body/antidbg stay off for this
				// preset), gets printed, and is embedded inside the
				// interpreter-definition handler of the scaffold:
				//
				//   return setmetatable({
				//     <P2 fields...>,
				//     <init>  = initializer (return true, s0, nil×10),
				//     <mul>/<mod> = arithmetic assistants (iL/vL shape),
				//     <g_k>   = per-carrier staging handlers (checksum
				//               folded through the assistants),
				//     <dh>    = interpreter-definition handler:
				//               <passed interpreter> + entry closure,
				//     <ctl>   = control handler (2 = done / 1 = continue),
				//     <loop>  = CPS loop: range tree + `continue` leaves,
				//     <fc>    = top machine (FC shape): initializer +
				//               while-flag loop + control-code return,
				//   }, {}):<fc>()(...);
				//
				// Remaining P3 work: scaling the handler chain (splitting
				// the decode pipeline) + inlining the execution loops
				// into dual numeric-slot runners (docs plan §P3 A/B).
				let mut ttable = symtab::resolve(&mut tblock);
				let mut reserved = mangle::reserved_set(
					&ttable
						.globals
						.iter()
						.cloned()
						.collect::<std::collections::HashSet<_>>(),
				);
				reserved.extend(ttable.syms.iter().map(|s| s.name.clone()));
				if opts.do_junk {
					junk::inject(&mut tblock, &mut ttable, &mut rng, 2);
				}
				if opts.do_mangle {
					// scaffold param names used inside the wrapper
					// functions must never be shadowed by mangled
					// interpreter locals (they share the def-runner
					// scope after wrapping)
					reserved.extend(
						["b", "C", "p1", "p2", "p3", "E", "V"]
							.iter()
							.map(|s| s.to_string()),
					);
					mangle::mangle(&mut ttable, &mut rng, true);
				}
				if opts.do_strings {
					strings::apply_strings(
						&mut tblock, &mut ttable, &mut rng, &reserved,
					);
				}
				if opts.do_numbers {
					numbers::apply_numbers(&mut tblock, &mut rng);
				}
				// Locate the VM local's post-mangle name (the definition
				// handler's entry closure must call it by that name).
				let mut vm_name = String::from("VM");
				for st in tblock.stmts.iter() {
					if let ast::Stmt::Local { names, syms, .. } = st {
						if names.first().map(|s| s.as_str()) == Some("VM") {
							if let Some(id) = syms.first() {
								vm_name = ttable.syms[*id as usize]
									.name
									.clone();
							}
						}
					}
				}
				let interp_src = printer::print_chunk(&ttable, &tblock);
				// P3-B: the interpreter lives in a numeric-slot runner
				// ([r1], sample [73] shape); the user-facing entry is
				// routed through a second numeric-slot runner ([r2],
				// sample [18] shape). Reserve both slots so P2's
				// primitive-slot draw can't collide with them.
				let mut slot_pool: Vec<i64> = (1..=126).collect();
				rng.shuffle(&mut slot_pool);
				let r1 = slot_pool[0];
				let r2 = slot_pool[1];
				let ks = slot_pool[2]; // LCG keystream state slot
				let kg = slot_pool[3]; // LCG keystream generator slot
				// decoy LCG slots (F28): clear of every other slot
				// mechanism (runners/keystream/primitives/AL/context
				// slots AND the Nop self-mod instruction positions)
				let max_len = carrier_bytes.iter().map(|c| c.len()).max().unwrap_or(0);
				let (d1, d2) = vmgen::v15::decoy_slots(max_len, &[r1, r2, ks, kg]);
				let (scaffold_fields, fc) = vmgen::v15::scaffold(
					&mut rng,
					&interp_src,
					&vm_name,
					&carrier_bytes,
					r1,
					r2,
					ks,
					kg,
					d1,
					d2,
					&program.carrier,
					opts.do_guard,
				);
				let mut fields =
					vmgen::v15::module_fields(&mut rng, &[r1, r2, ks, kg, d1, d2]);
				fields.extend(scaffold_fields);
				rng.shuffle(&mut fields);
				let module = ast::Expr::Table { fields };
				let shell = ast::Stmt::Return(vec![ast::Expr::Call {
					func: Box::new(ast::Expr::Method {
						obj: Box::new(ast::Expr::Call {
							func: Box::new(ast::Expr::Ident {
								name: "setmetatable".to_string(),
								sym: None,
							}),
							args: vec![
								module,
								ast::Expr::Table { fields: Vec::new() },
							],
						}),
						name: fc,
						args: Vec::new(),
					}),
					args: vec![ast::Expr::Vararg],
				}]);
				let mut shell_block = ast::Block { stmts: vec![shell] };
				let shell_table = symtab::resolve(&mut shell_block);
				(shell_block, shell_table, true)
			} else {
				let vm_call = ast::Expr::Call {
					func: Box::new(ast::Expr::Ident {
						name: "VM".to_string(),
						sym: None,
					}),
					args: carrier_bytes
						.into_iter()
						.map(|b| ast::Expr::Str {
							bytes: b,
							is_binary: true,
						})
						.collect(),
				};
				tblock.stmts.push(ast::Stmt::ExprStmt(vm_call));
				let ttable = symtab::resolve(&mut tblock);
				(tblock, ttable, false)
			}
		} else {
			(block, table, false)
		};
		// pipeline: junk -> mangle -> flatten -> strings
		if opts.do_vm && vm_raw {
			// debug: emit the raw (unobfuscated) VM container
		let out = printer::print_chunk(&table, &block);
			return Ok(out);
		}
		let tim = std::env::var("LURAPH_TIMING").is_ok();
		let mut tmark = std::time::Instant::now();
		let mut tlog = |name: &str| {
			if tim {
				eprintln!("[time] {} = {:?}", name, tmark.elapsed());
				tmark = std::time::Instant::now();
			}
		};
		// v15 shell: passes already ran over the interpreter block before
		// the module-table wrap (see above); re-running them here would
		// double-mangle / hoist loaders out of the boot handler.
		let mut reserved = std::collections::HashSet::new();
		if !v15_done {
			if opts.do_junk {
				junk::inject(&mut block, &mut table, &mut rng, 2);
				tlog("junk");
			}
			if opts.do_mangle {
				mangle::mangle(&mut table, &mut rng, false);
				tlog("mangle");
			}
			reserved = mangle::reserved_set(
				&table.globals.iter().cloned().collect::<std::collections::HashSet<_>>(),
			);
			reserved.extend(table.syms.iter().map(|s| s.name.clone()));
			// L3 flatten bloats the VM template's big dispatch if/elseif
			// tree into a state machine that exceeds Lua 5.1's
			// 200-local-variable limit, so skip it when the VM is enabled
			// (the VM template is still obfuscated by
			// mangle/strings/numbers/body/antidbg).
			if opts.do_flatten && !opts.do_vm {
				flatten::flatten_block(&mut block, &mut table, &mut rng, &mut reserved);
				tlog("flatten");
			}
			if opts.do_strings {
				strings::apply_strings(&mut block, &mut table, &mut rng, &reserved);
				tlog("strings");
			}
			if opts.do_numbers {
				numbers::apply_numbers(&mut block, &mut rng);
				tlog("numbers");
			}
			if opts.do_body {
				body::apply_body(&mut block, &mut table, &mut rng, &mut reserved, luau);
				tlog("body");
			}
			if opts.do_antidbg {
				antidbg::apply_antidbg(&mut block, &mut table, &mut rng, &mut reserved);
				tlog("antidbg");
			}
		}
		tlog("pre-print");
		// Luau targets print with compound-assignment folding (F24
		// sample shape); 5.1 keeps plain assignments
		let out = if luau {
			printer::print_chunk_luau(&table, &block)
		} else {
			printer::print_chunk(&table, &block)
		};
		tlog("print");
		if opts.do_minify {
			let m = minify::minify(&out, luau).map_err(|e| format!("minify: {}", e))?;
			tlog("minify");
			if opts.do_v15 {
				// F1/F2 shape: header comment + blank line + one-line body
				// + trailing semicolon, no trailing newline (sample form).
				Ok(format!(
					"-- This file was protected using luraph v{}\n\n{};",
					VERSION,
					m.trim_end_matches('\n')
				))
			} else if opts.do_guard {
				// anti-debug guard prelude ahead of the payload
				Ok(format!("{}\n{}", guard::guard_prelude(&mut rng, luau), m))
			} else {
				Ok(m)
			}
		} else if opts.do_guard && !opts.do_v15 {
			Ok(format!("{}\n{}", guard::guard_prelude(&mut rng, luau), out))
		} else {
			Ok(out)
		}
	})();

	match result {
		Ok(out) => match &opts.output {
			Some(path) => match std::fs::write(path, out) {
				Ok(()) => {}
				Err(e) => {
					eprintln!("error: cannot write {}: {}", path, e);
					return ExitCode::FAILURE;
				}
			},
			None => print!("{}", out),
		},
		Err(e) => {
			eprintln!("luraph-rs: {}", e);
			return ExitCode::FAILURE;
		}
	}
	ExitCode::SUCCESS
}
