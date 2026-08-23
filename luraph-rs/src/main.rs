//! luraph-rs — commercial-grade Lua 5.1 / Luau obfuscator.
//!
//! M0 (foundation): parse -> resolve -> print (round-trip, no obfuscation
//! passes yet). Later milestones add: mangle/strings/flatten/junk/numbers/
//! body/vmgen passes and presets.

mod ast;
mod lexer;
mod parser;
mod printer;
mod rng;
mod symtab;

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
	dialect: &'static str, // "5.1" | "luau"
	input: String,
	output: Option<String>,
}

fn print_help() {
	print!(
		"luraph-rs {} — commercial-grade Lua 5.1 / Luau obfuscator
Usage: luraph-rs [options] <input.lua> [output.lua]

Options:
  --dialect <5.1|luau>   target dialect (default: 5.1)
  -o, --output <file>    output file (default: stdout)
  -h, --help             show this help
  --version              show version

M0: round-trip pipeline (parse -> resolve -> print), no obfuscation passes.
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

	let src = match std::fs::read_to_string(&opts.input) {
		Ok(s) => s,
		Err(e) => {
			eprintln!("error: cannot read {}: {}", opts.input, e);
			return ExitCode::FAILURE;
		}
	};

	let result = (|| -> Result<String, String> {
		let mut block = parser::parse(&src, luau).map_err(|e| e.to_string())?;
		let table = symtab::resolve(&mut block);
		let out = printer::print_chunk(&table, &block);
		Ok(out)
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
