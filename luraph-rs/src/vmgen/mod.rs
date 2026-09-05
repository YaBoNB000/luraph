//! L6 — custom VM: the core moat.
//!
//! The user program is compiled to private bytecode (register VM,
//! per-build opcode permutation) which is executed by an interpreter
//! generated at build time — the interpreter itself is just Lua source
//! that runs through the full obfuscation pipeline, so the output is
//! [obfuscated interpreter] + [encrypted bytecode] with no natively
//! readable program structure.

pub mod compiler;
pub mod handlers;
pub mod isa;
pub mod strpool;
pub mod template;
pub mod v15;

pub use compiler::compile;

/// 增量⑩ (防静态, 报告突破口 #5/#2): build-time key manifest. When
/// LURAPH_KEY_MANIFEST points at a file, every secret emitted by the
/// codegen is recorded there so tests/key_literal_check.py can prove
/// NONE of them survives as a bare literal in the output.
pub(crate) fn manifest_key(name: &str, value: u64) {
	if let Ok(p) = std::env::var("LURAPH_KEY_MANIFEST") {
		use std::io::Write;
		if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
			let _ = writeln!(f, "{name}={value}");
		}
	}
}
