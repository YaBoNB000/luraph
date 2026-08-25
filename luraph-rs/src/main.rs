//! luraph-rs — commercial-grade Lua 5.1 / Luau obfuscator.
//!
//! Pipeline: parse -> symtab -> [junk] -> [mangle] -> [flatten] -> [strings] -> print
//! (later: numbers/body/antidbg/vmgen)

mod antidbg;
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
	do_vm: bool,
	do_strings: bool,
	do_flatten: bool,
	do_junk: bool,
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
  --minify               L1 minify output to a single compact line (default: enabled)
  --no-minify            keep the normalized (indented) printer output
  --no-numbers           disable L4 numeric literal rewriting (default: enabled)
  --no-body              disable L5 whole-program encryption (default: enabled)
  --no-antidbg           disable L7 anti-tamper (default: enabled)
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
		do_vm: false,
		do_strings: true,
		do_flatten: true,
		do_junk: true,
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
			"--no-mangle" => opts.do_mangle = false,
		"--minify" => opts.do_minify = true,
		"--no-minify" => opts.do_minify = false,
		"--no-numbers" => opts.do_numbers = false,
		"--no-body" => opts.do_body = false,
		"--no-antidbg" => opts.do_antidbg = false,
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
		let (mut block, mut table) = if opts.do_vm {
			let program = vmgen::compile(&block, &table, &mut rng, !luau);
			let n = program.fns.len();
			let tsrc = vmgen::template::generate(&program.opmap, &program.slot_perm, &mut rng, n);
			if std::env::var("LURAPH_VM_TSRC").is_ok() {
				std::fs::write("/tmp/vm_tsrc.lua", &tsrc).unwrap();
			}
			let mut tblock = parser::parse(&tsrc, luau).map_err(|e| e.to_string())?;
			let args: Vec<ast::Expr> = program
				.fns
				.iter()
				.map(|b| ast::Expr::Str {
					bytes: b.clone(),
					is_binary: true,
				})
				.collect();
			tblock.stmts.push(ast::Stmt::ExprStmt(ast::Expr::Call {
				func: Box::new(ast::Expr::Ident {
					name: "VM".to_string(),
					sym: None,
				}),
				args,
			}));
			let ttable = symtab::resolve(&mut tblock);
			(tblock, ttable)
		} else {
			(block, table)
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
		if opts.do_junk {
			junk::inject(&mut block, &mut table, &mut rng, 2);
			tlog("junk");
		}
		if opts.do_mangle {
			mangle::mangle(&mut table, &mut rng);
			tlog("mangle");
		}
		let mut reserved = mangle::reserved_set(
			&table.globals.iter().cloned().collect::<std::collections::HashSet<_>>(),
		);
		reserved.extend(table.syms.iter().map(|s| s.name.clone()));
		// L3 flatten bloats the VM template's big dispatch if/elseif tree
		// into a state machine that exceeds Lua 5.1's 200-local-variable
		// limit, so skip it when the VM is enabled (the VM template is
		// still obfuscated by mangle/strings/numbers/body/antidbg).
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
		tlog("pre-print");
		let out = printer::print_chunk(&table, &block);
		tlog("print");
		if opts.do_minify {
			let m = minify::minify(&out, luau).map_err(|e| format!("minify: {}", e))?;
			tlog("minify");
			Ok(m)
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
