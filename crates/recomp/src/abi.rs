//! The C ABI between a generated library and the runtime. Title-neutral: a table of
//! (guest address, entry) pairs. A guest address the table lacks is served by the interpreter.

/// Version of this ABI; the generated library reports it and the loader refuses a mismatch.
pub const ABI_VERSION: u32 = 1;

/// C declarations every generated library includes. Entries take the register file by pointer
/// (`uu_regs`), so no per-title argument convention enters the shared layer.
pub const HEADER: &str = "\
#include <stddef.h>
#include <stdint.h>
typedef struct { uint64_t gpr[16]; uint64_t rflags; } uu_regs;
typedef void (*uu_entry)(uu_regs *);
typedef struct { uint64_t guest_addr; uu_entry entry; } uu_table_entry;
uint32_t uu_recomp_abi_version(void);
const uu_table_entry *uu_recomp_table(size_t *count);
";

#[cfg(test)]
mod tests {
    #[test]
    fn header_names_the_exports() {
        assert!(super::HEADER.contains("uu_recomp_abi_version"));
        assert!(super::HEADER.contains("uu_recomp_table"));
    }
}
