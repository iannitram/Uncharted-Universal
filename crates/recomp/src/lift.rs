//! Generator for a small authored x86-64 subset: `mov eax, imm32` (B8), `add eax, imm32` (05),
//! `nop` (90) and `ret` (C3). Anything else is refused with its offset, never guessed. This is a
//! stand-in for the full decoder (not vendored yet); it exists so the generator, the ABI and the
//! coverage counts run in CI on authored bytes.

use crate::abi;
use crate::coverage::Coverage;
use std::fmt::Write as _;

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Unsupported { offset: usize, opcode: u8 },
    Truncated { offset: usize },
    NoReturn,
}

/// One guest function: its address and bytes.
pub struct Function<'a> {
    pub addr: u64,
    pub code: &'a [u8],
}

/// Lift one function to the body of a C entry; the result is left in `gpr[0]` (rax).
pub fn lift(code: &[u8]) -> Result<String, Refusal> {
    let mut body = String::new();
    let mut i = 0;
    while i < code.len() {
        match code[i] {
            0x90 => i += 1,
            0xC3 => {
                body.push_str("    return;\n");
                return Ok(body);
            }
            op @ (0xB8 | 0x05) => {
                let b = code
                    .get(i + 1..i + 5)
                    .ok_or(Refusal::Truncated { offset: i })?;
                let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                let _ = if op == 0xB8 {
                    writeln!(body, "    r->gpr[0] = (uint32_t){v}u;")
                } else {
                    writeln!(body, "    r->gpr[0] = (uint32_t)(r->gpr[0] + {v}u);")
                };
                i += 5;
            }
            opcode => return Err(Refusal::Unsupported { offset: i, opcode }),
        }
    }
    Err(Refusal::NoReturn)
}

pub struct Generated {
    pub c_source: String,
    pub coverage: Coverage,
    /// Functions refused (served by the interpreter), with the reason.
    pub refused: Vec<(u64, Refusal)>,
}

/// Generate one C translation unit. `executed` is the count of functions observed executing;
/// functions the generator refuses stay out of the table and fall to the interpreter.
pub fn generate(funcs: &[Function<'_>]) -> Generated {
    let mut src = String::from(abi::HEADER);
    let mut table = String::new();
    let mut refused = Vec::new();
    let mut library = 0u64;
    for f in funcs {
        match lift(f.code) {
            Ok(body) => {
                let _ = write!(
                    src,
                    "static void fn_{:x}(uu_regs *r) {{\n{body}}}\n",
                    f.addr
                );
                let _ = writeln!(table, "    {{ 0x{:x}ull, fn_{:x} }},", f.addr, f.addr);
                library += 1;
            }
            Err(e) => refused.push((f.addr, e)),
        }
    }
    let _ = write!(
        src,
        "static const uu_table_entry table[] = {{\n{table}    {{ 0, 0 }}\n}};\n\
         uint32_t uu_recomp_abi_version(void) {{ return {}; }}\n\
         const uu_table_entry *uu_recomp_table(size_t *count) {{ *count = {library}; return table; }}\n",
        abi::ABI_VERSION
    );
    Generated {
        c_source: src,
        coverage: Coverage {
            executed: funcs.len() as u64,
            library,
        },
        refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_and_refuses() {
        let ok = [0xB8, 40, 0, 0, 0, 0x05, 2, 0, 0, 0, 0xC3];
        let bad = [0x90, 0x0F, 0x05];
        let cut = [0xB8, 1];
        let noret = [0x90];
        let g = generate(&[
            Function {
                addr: 0x10,
                code: &ok,
            },
            Function {
                addr: 0x20,
                code: &bad,
            },
            Function {
                addr: 0x30,
                code: &cut,
            },
            Function {
                addr: 0x40,
                code: &noret,
            },
        ]);
        assert_eq!(
            g.coverage,
            Coverage {
                executed: 4,
                library: 1
            }
        );
        assert_eq!(
            g.refused,
            vec![
                (
                    0x20,
                    Refusal::Unsupported {
                        offset: 1,
                        opcode: 0x0F
                    }
                ),
                (0x30, Refusal::Truncated { offset: 0 }),
                (0x40, Refusal::NoReturn),
            ]
        );
        assert!(g.c_source.contains("fn_10") && !g.c_source.contains("fn_20"));
        assert!(g.c_source.contains("*count = 1;"));
    }

    #[test]
    fn generated_c_compiles_and_runs() {
        let dir = std::env::temp_dir().join("uu_recomp_lift_test");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let ok = [0xB8, 40, 0, 0, 0, 0x05, 2, 0, 0, 0, 0xC3];
        let g = generate(&[Function {
            addr: 0x10,
            code: &ok,
        }]);
        let main = "\nint main(void){size_t n;const uu_table_entry*t=uu_recomp_table(&n);\
                    uu_regs r={{0}};t[0].entry(&r);return (n==1&&t[0].guest_addr==0x10&&r.gpr[0]==42&&uu_recomp_abi_version()==1)?0:1;}\n";
        let c = dir.join("t.c");
        if std::fs::write(&c, format!("{}{main}", g.c_source)).is_err() {
            return;
        }
        let exe = dir.join("t");
        let built = std::process::Command::new("cc")
            .arg(&c)
            .arg("-o")
            .arg(&exe)
            .status();
        match built {
            Ok(s) if s.success() => {
                let run = std::process::Command::new(&exe).status();
                assert!(run.is_ok_and(|s| s.success()));
            }
            Ok(_) => panic!("generated C did not compile"),
            Err(_) => {} // no C compiler on this runner
        }
    }
}
