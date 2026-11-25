//! Authored x86-64 code for the recompiler tests: a tiny plain ELF64 built here, byte by byte.
//! Nothing in it comes from a game. Used by crates/recomp/tests/e2e.rs.
//!
//! Layout: one PT_LOAD (read, execute) with file offset == virtual address, whose first bytes
//! are the ELF and program headers, then the functions (16-byte aligned), a jump table, an
//! import slot, and a `.eh_frame_hdr` table with a frame description for every function but
//! `ADD3` (a leaf the scan has to find by following the call to it, as with real leaf code).

#![allow(dead_code)]

/// The functions, by name: image offsets and sizes, filled by [`build`].
#[derive(Debug, Clone, Copy)]
pub struct Function {
    pub name: &'static str,
    pub offset: u64,
    pub size: u64,
}

pub struct Fixture {
    pub elf: Vec<u8>,
    pub functions: Vec<Function>,
    /// Where the guest's import stub jumps through (8 bytes the host fills with a function).
    pub import_slot: u64,
}

impl Fixture {
    pub fn offset(&self, name: &str) -> u64 {
        self.function(name).offset
    }
    pub fn function(&self, name: &str) -> Function {
        *self
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no function {name}"))
    }
}

/// A function body under construction: bytes, plus fix-ups for rel32 references by name.
struct Body {
    name: &'static str,
    code: Vec<u8>,
    /// (position of a rel32 in `code`, target function name): rel32 is relative to the end of
    /// the 4 bytes.
    calls: Vec<(usize, &'static str)>,
    /// (position of a rel32, which data item): same, to the jump table or the import slot.
    data: Vec<(usize, Data)>,
    /// Whether the function gets a frame description.
    unwind: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Data {
    SwitchTable,
    ImportSlot,
}

fn body(name: &'static str, unwind: bool, code: &[u8]) -> Body {
    Body {
        name,
        code: code.to_vec(),
        calls: Vec::new(),
        data: Vec::new(),
        unwind,
    }
}

fn align(value: usize, to: usize) -> usize {
    value.div_ceil(to) * to
}

/// The bodies, in image order. Expected results are computed by the tests in Rust, never by
/// running this code on bbcpu.
fn bodies() -> Vec<Body> {
    let mut all = Vec::new();

    // add3(a, b, c) = a + b + c: lea rax,[rdi+rsi]; add rax,rdx; ret
    all.push(body(
        "add3",
        false,
        &[0x48, 0x8d, 0x04, 0x37, 0x48, 0x01, 0xd0, 0xc3],
    ));

    // sum_to(n) = 1 + 2 + ... + n: xor eax,eax; test rdi,rdi; jz end; L: add rax,rdi; dec rdi; jnz L; end: ret
    all.push(body(
        "sum_to",
        true,
        &[
            0x31, 0xc0, 0x48, 0x85, 0xff, 0x74, 0x08, 0x48, 0x01, 0xf8, 0x48, 0xff, 0xcf, 0x75,
            0xf8, 0xc3,
        ],
    ));

    // call_chain(a, b, c) = add3(a, b, c) + 1
    let mut chain = body(
        "call_chain",
        true,
        &[
            0x48, 0x83, 0xec, 0x08, // sub rsp,8
            0xe8, 0, 0, 0, 0, // call add3
            0x48, 0x83, 0xc0, 0x01, // add rax,1
            0x48, 0x83, 0xc4, 0x08, // add rsp,8
            0xc3,
        ],
    );
    chain.calls.push((5, "add3"));
    all.push(chain);

    // switch4(i) = [10, 20, 30, 40][i] for i <= 3, else 0xffffffff (a compiler-style jump table)
    let mut switch = body(
        "switch4",
        true,
        &[
            0x83, 0xff, 0x03, // cmp edi,3
            0x77, 0x00, // ja default (patched below)
            0x48, 0x8d, 0x0d, 0, 0, 0, 0, // lea rcx,[rip+table]
            0x48, 0x63, 0x04, 0xb9, // movsxd rax,dword [rcx+rdi*4]
            0x48, 0x01, 0xc8, // add rax,rcx
            0xff, 0xe0, // jmp rax
        ],
    );
    switch.data.push((8, Data::SwitchTable));
    all.push(switch);

    // square_f(n) = trunc((double)n * (double)n): SSE2
    all.push(body(
        "square_f",
        true,
        &[
            0xf2, 0x48, 0x0f, 0x2a, 0xc7, // cvtsi2sd xmm0,rdi
            0xf2, 0x0f, 0x59, 0xc0, // mulsd xmm0,xmm0
            0xf2, 0x48, 0x0f, 0x2c, 0xc0, // cvttsd2si rax,xmm0
            0xc3,
        ],
    ));

    // bump(p, k): *p += k; returns the new value. Reads and writes guest memory.
    all.push(body(
        "bump",
        true,
        &[0x48, 0x8b, 0x07, 0x48, 0x01, 0xf0, 0x48, 0x89, 0x07, 0xc3],
    ));

    // import_stub: jmp qword [rip+slot]. The scan sees a one-instruction function that jumps
    // through a slot, which imports.tsv names as an import when a relocation says so.
    let mut stub = body("import_stub", true, &[0xff, 0x25, 0, 0, 0, 0]);
    stub.data.push((2, Data::ImportSlot));
    all.push(stub);

    // call_import(a, b) = import_stub(a, b)
    let mut caller = body(
        "call_import",
        true,
        &[
            0x48, 0x83, 0xec, 0x08, 0xe8, 0, 0, 0, 0, 0x48, 0x83, 0xc4, 0x08, 0xc3,
        ],
    );
    caller.calls.push((5, "import_stub"));
    all.push(caller);

    // pdep_user(a, b): pdep rax,rdi,rsi (BMI2: neither bbcpu's interpreter nor the generator
    // knows it; the PS4's CPU has no BMI2), then ret.
    all.push(body(
        "pdep_user",
        true,
        &[0xc4, 0xe2, 0xc3, 0xf5, 0xc6, 0xc3],
    ));

    all
}

/// Build the ELF.
pub fn build() -> Fixture {
    let mut bodies = bodies();
    // Place the functions.
    let mut at = 0x100usize;
    let mut placed: Vec<(usize, usize)> = Vec::new(); // (offset, size incl. switch cases)

    // The switch function gets its cases appended before layout.
    let case_values = [10u32, 20, 30, 40];
    let mut case_starts: Vec<usize> = Vec::new(); // positions inside the switch body
    for b in bodies.iter_mut() {
        if b.name != "switch4" {
            continue;
        }
        for value in case_values {
            case_starts.push(b.code.len());
            b.code.push(0xb8); // mov eax,imm32
            b.code.extend_from_slice(&value.to_le_bytes());
            b.code.push(0xc3);
        }
        let default_at = b.code.len();
        b.code
            .extend_from_slice(&[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]); // mov eax,-1; ret
        b.code[4] = (default_at - 5) as u8; // ja rel8 is relative to the end of the ja
    }
    for b in &bodies {
        at = align(at, 16);
        placed.push((at, b.code.len()));
        at += b.code.len();
    }
    // The jump table, then the import slot.
    let table_at = align(at, 4);
    let table_len = case_values.len() * 4;
    let slot_at = align(table_at + table_len, 8);
    let data_end = slot_at + 8;
    let hdr_at = align(data_end, 8);

    let offset_of = |name: &str| -> usize {
        let index = bodies.iter().position(|b| b.name == name).unwrap();
        placed[index].0
    };

    // Resolve the rel32 fix-ups and write the image.
    let fde_functions: Vec<usize> = bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| b.unwind)
        .map(|(i, _)| i)
        .collect();
    let fde_size = 20;
    let hdr_len = 12 + 8 * fde_functions.len();
    let fde_at = align(hdr_at + hdr_len, 8);
    let total = fde_at + fde_size * fde_functions.len();
    let mut image = vec![0u8; align(total, 16)];

    for (index, b) in bodies.iter().enumerate() {
        let start = placed[index].0;
        let mut code = b.code.clone();
        for (position, target) in &b.calls {
            let rel = offset_of(target) as i64 - (start + position + 4) as i64;
            code[*position..position + 4].copy_from_slice(&(rel as i32).to_le_bytes());
        }
        for (position, data) in &b.data {
            let target = match data {
                Data::SwitchTable => table_at,
                Data::ImportSlot => slot_at,
            };
            let rel = target as i64 - (start + position + 4) as i64;
            code[*position..position + 4].copy_from_slice(&(rel as i32).to_le_bytes());
        }
        image[start..start + code.len()].copy_from_slice(&code);
    }
    let switch_start = offset_of("switch4");
    for (i, case) in case_starts.iter().enumerate() {
        let rel = (switch_start + case) as i64 - table_at as i64;
        image[table_at + 4 * i..table_at + 4 * i + 4].copy_from_slice(&(rel as i32).to_le_bytes());
    }

    // .eh_frame_hdr: version, eh_frame_ptr encoding, fde_count encoding (udata4), table encoding
    // (datarel sdata4), eh_frame_ptr, fde_count, then (initial location, FDE) pairs relative to
    // the table's own address.
    image[hdr_at] = 1;
    image[hdr_at + 1] = 0x1b;
    image[hdr_at + 2] = 0x03;
    image[hdr_at + 3] = 0x3b;
    image[hdr_at + 4..hdr_at + 8]
        .copy_from_slice(&((fde_at as i64 - (hdr_at + 4) as i64) as i32).to_le_bytes());
    image[hdr_at + 8..hdr_at + 12].copy_from_slice(&(fde_functions.len() as u32).to_le_bytes());
    for (n, index) in fde_functions.iter().enumerate() {
        let (start, size) = placed[*index];
        let entry = hdr_at + 12 + 8 * n;
        image[entry..entry + 4]
            .copy_from_slice(&((start as i64 - hdr_at as i64) as i32).to_le_bytes());
        let fde = fde_at + fde_size * n;
        image[entry + 4..entry + 8]
            .copy_from_slice(&((fde as i64 - hdr_at as i64) as i32).to_le_bytes());
        // FDE: length, CIE pointer, pc_begin (pcrel sdata4), pc_range, augmentation length, padding.
        image[fde..fde + 4].copy_from_slice(&16u32.to_le_bytes());
        image[fde + 4..fde + 8].copy_from_slice(&(fde as u32 + 4).to_le_bytes());
        image[fde + 8..fde + 12]
            .copy_from_slice(&((start as i64 - (fde + 8) as i64) as i32).to_le_bytes());
        // The range excludes the case blocks' table: functions end at their last byte.
        image[fde + 12..fde + 16].copy_from_slice(&(size as u32).to_le_bytes());
    }

    // ELF header and two program headers.
    let file_size = image.len() as u64;
    image[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    put(16, &2u16.to_le_bytes()); // ET_EXEC
    put(18, &62u16.to_le_bytes()); // x86-64
    put(20, &1u32.to_le_bytes());
    put(24, &(offset_of("add3") as u64).to_le_bytes()); // entry
    put(32, &64u64.to_le_bytes()); // phoff
    put(52, &64u16.to_le_bytes()); // ehsize
    put(54, &56u16.to_le_bytes()); // phentsize
    put(56, &2u16.to_le_bytes()); // phnum
    let mut ph = |index: usize, kind: u32, flags: u32, offset: u64, size: u64| {
        let at = 64 + 56 * index;
        put(at, &kind.to_le_bytes());
        put(at + 4, &flags.to_le_bytes());
        put(at + 8, &offset.to_le_bytes());
        put(at + 16, &offset.to_le_bytes()); // vaddr == offset
        put(at + 24, &offset.to_le_bytes());
        put(at + 32, &size.to_le_bytes());
        put(at + 40, &size.to_le_bytes());
        put(at + 48, &0x1000u64.to_le_bytes());
    };
    ph(0, 1, 5, 0, file_size); // PT_LOAD, R+X, the whole file
    ph(
        1,
        0x6474_e550,
        4,
        hdr_at as u64,
        (fde_at + fde_size * fde_functions.len() - hdr_at) as u64,
    ); // PT_GNU_EH_FRAME

    let functions = bodies
        .iter()
        .enumerate()
        .map(|(i, b)| Function {
            name: b.name,
            offset: placed[i].0 as u64,
            size: placed[i].1 as u64,
        })
        .collect();
    Fixture {
        elf: image,
        functions,
        import_slot: slot_at as u64,
    }
}
