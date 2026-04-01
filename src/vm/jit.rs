// Тризуб JIT — Bytecode → x86_64 машинний код
// Компілює bytecode інструкції в нативний код, виконує через VirtualAlloc/mmap

use super::bytecode::*;

#[cfg(target_arch = "x86_64")]
pub struct JitCompiler {
    code: Vec<u8>,
    // Патчі для переходів (offset в code → target instruction index)
    jump_patches: Vec<(usize, usize)>,
    // Маппінг: instruction index → offset в code
    inst_offsets: Vec<usize>,
}

#[cfg(target_arch = "x86_64")]
impl Default for JitCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl JitCompiler {
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(4096),
            jump_patches: Vec::new(),
            inst_offsets: Vec::new(),
        }
    }

    pub fn compile(mut self, chunk: &Chunk) -> JitFunction {
        // Prologue: push rbp; mov rbp, rsp; sub rsp, locals*8
        self.emit(&[0x55]); // push rbp
        self.emit(&[0x48, 0x89, 0xE5]); // mov rbp, rsp
        let locals_size = ((chunk.local_count + 1) * 8) as u32;
        // sub rsp, locals_size (aligned to 16)
        let aligned = (locals_size + 15) & !15;
        self.emit(&[0x48, 0x81, 0xEC]); // sub rsp, imm32
        self.emit_u32(aligned);

        // Compile each instruction
        for (i, inst) in chunk.code.iter().enumerate() {
            self.inst_offsets.push(self.code.len());
            match inst.op {
                Op::Const => {
                    let val = match &chunk.constants[inst.arg as usize] {
                        BcValue::Int(n) => *n,
                        BcValue::Float(f) => *f as i64,
                        BcValue::Bool(b) => *b as i64,
                        _ => 0,
                    };
                    // mov rax, imm64; push rax
                    self.emit(&[0x48, 0xB8]); // mov rax, imm64
                    self.emit_i64(val);
                    self.emit(&[0x50]); // push rax
                }
                Op::Pop => {
                    self.emit(&[0x48, 0x83, 0xC4, 0x08]); // add rsp, 8
                }
                Op::LoadLocal => {
                    let offset = (inst.arg + 1) * 8;
                    // mov rax, [rbp - offset]; push rax
                    self.emit(&[0x48, 0x8B, 0x85]); // mov rax, [rbp - disp32]
                    self.emit_u32((!offset).wrapping_add(1)); // negative offset as u32
                    self.emit(&[0x50]); // push rax
                }
                Op::StoreLocal => {
                    let offset = (inst.arg + 1) * 8;
                    // pop rax; mov [rbp - offset], rax
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x89, 0x85]); // mov [rbp - disp32], rax
                    self.emit_u32((!offset).wrapping_add(1));
                }
                Op::Add => {
                    // pop rcx; pop rax; add rax, rcx; push rax
                    self.emit(&[0x59]); // pop rcx
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x01, 0xC8]); // add rax, rcx
                    self.emit(&[0x50]); // push rax
                }
                Op::Sub => {
                    self.emit(&[0x59, 0x58]); // pop rcx; pop rax
                    self.emit(&[0x48, 0x29, 0xC8]); // sub rax, rcx
                    self.emit(&[0x50]);
                }
                Op::Mul => {
                    self.emit(&[0x59, 0x58]); // pop rcx; pop rax
                    self.emit(&[0x48, 0x0F, 0xAF, 0xC1]); // imul rax, rcx
                    self.emit(&[0x50]);
                }
                Op::Div => {
                    self.emit(&[0x59, 0x58]); // pop rcx; pop rax
                    self.emit(&[0x48, 0x99]); // cqo (sign extend rax → rdx:rax)
                    self.emit(&[0x48, 0xF7, 0xF9]); // idiv rcx
                    self.emit(&[0x50]);
                }
                Op::Mod => {
                    self.emit(&[0x59, 0x58]); // pop rcx; pop rax
                    self.emit(&[0x48, 0x99]); // cqo
                    self.emit(&[0x48, 0xF7, 0xF9]); // idiv rcx
                    self.emit(&[0x52]); // push rdx (remainder)
                }
                Op::Neg => {
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0xF7, 0xD8]); // neg rax
                    self.emit(&[0x50]);
                }
                Op::Eq => { self.emit_cmp(0x94); } // sete
                Op::Ne => { self.emit_cmp(0x95); } // setne
                Op::Lt => { self.emit_cmp(0x9C); } // setl
                Op::Le => { self.emit_cmp(0x9E); } // setle
                Op::Gt => { self.emit_cmp(0x9F); } // setg
                Op::Ge => { self.emit_cmp(0x9D); } // setge
                Op::Inc => {
                    let offset = (inst.arg + 1) * 8;
                    // inc qword [rbp - offset]
                    self.emit(&[0x48, 0xFF, 0x85]); // inc [rbp - disp32]
                    self.emit_u32((!offset).wrapping_add(1));
                }
                Op::AddAssign => {
                    let offset = (inst.arg + 1) * 8;
                    // pop rax; add [rbp - offset], rax
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x01, 0x85]); // add [rbp - disp32], rax
                    self.emit_u32((!offset).wrapping_add(1));
                }
                Op::Jump => {
                    // jmp rel32 (patch later)
                    self.emit(&[0xE9]);
                    self.jump_patches.push((self.code.len(), inst.arg as usize));
                    self.emit_u32(0);
                }
                Op::JumpIfFalse => {
                    // pop rax; test rax, rax; je rel32
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x85, 0xC0]); // test rax, rax
                    self.emit(&[0x0F, 0x84]); // je rel32
                    self.jump_patches.push((self.code.len(), inst.arg as usize));
                    self.emit_u32(0);
                }
                Op::Loop => {
                    // Calculate backward jump target
                    let target_inst = i + 1 - inst.arg as usize;
                    self.emit(&[0xE9]); // jmp rel32
                    self.jump_patches.push((self.code.len(), target_inst));
                    self.emit_u32(0);
                }
                Op::Print => {
                    // pop rax → rcx (first arg on Windows x64)
                    // call print_i64 (address passed in r15)
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x89, 0xC1]); // mov rcx, rax
                    // sub rsp, 32 (shadow space for Win64 ABI)
                    self.emit(&[0x48, 0x83, 0xEC, 0x20]);
                    // call r15
                    self.emit(&[0x41, 0xFF, 0xD7]); // call r15
                    // add rsp, 32
                    self.emit(&[0x48, 0x83, 0xC4, 0x20]);
                }
                Op::Halt => {
                    // pop rax (return value); epilogue
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x89, 0xEC]); // mov rsp, rbp
                    self.emit(&[0x5D]); // pop rbp
                    self.emit(&[0xC3]); // ret
                }
                _ => {
                    // Unsupported → push 0
                    self.emit(&[0x48, 0x31, 0xC0]); // xor rax, rax
                    self.emit(&[0x50]); // push rax
                }
            }
        }
        // Safety: add epilogue if no Halt
        self.emit(&[0x48, 0x31, 0xC0]); // xor rax, rax
        self.emit(&[0x48, 0x89, 0xEC]); // mov rsp, rbp
        self.emit(&[0x5D]); // pop rbp
        self.emit(&[0xC3]); // ret
        self.inst_offsets.push(self.code.len());

        // Patch jumps
        for (patch_offset, target_inst) in &self.jump_patches {
            let target_offset = self.inst_offsets[*target_inst];
            let rel = (target_offset as i32) - (*patch_offset as i32 + 4);
            let bytes = rel.to_le_bytes();
            self.code[*patch_offset] = bytes[0];
            self.code[*patch_offset + 1] = bytes[1];
            self.code[*patch_offset + 2] = bytes[2];
            self.code[*patch_offset + 3] = bytes[3];
        }

        JitFunction::new(self.code)
    }

    pub fn compile_to_bytes(mut self, chunk: &Chunk) -> Vec<u8> {
        // Bare-metal: no prologue frame (kernel starts flat)
        let locals_size = ((chunk.local_count + 1) * 8) as u32;
        let aligned = (locals_size + 15) & !15;
        self.emit(&[0x48, 0x81, 0xEC]); // sub rsp, imm32
        self.emit_u32(aligned);

        for (i, inst) in chunk.code.iter().enumerate() {
            self.inst_offsets.push(self.code.len());
            match inst.op {
                Op::Const => {
                    let val = match &chunk.constants[inst.arg as usize] {
                        BcValue::Int(n) => *n,
                        BcValue::Float(f) => *f as i64,
                        BcValue::Bool(b) => *b as i64,
                        _ => 0,
                    };
                    self.emit(&[0x48, 0xB8]);
                    self.emit_i64(val);
                    self.emit(&[0x50]);
                }
                Op::Pop => { self.emit(&[0x48, 0x83, 0xC4, 0x08]); }
                Op::LoadLocal => {
                    let offset = (inst.arg + 1) * 8;
                    self.emit(&[0x48, 0x8B, 0x84, 0x24]); // mov rax, [rsp + disp32]
                    self.emit_u32(offset);
                    self.emit(&[0x50]);
                }
                Op::StoreLocal => {
                    let offset = (inst.arg + 1) * 8;
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x89, 0x84, 0x24]); // mov [rsp + disp32], rax
                    self.emit_u32(offset);
                }
                Op::Add => { self.emit(&[0x59, 0x58, 0x48, 0x01, 0xC8, 0x50]); }
                Op::Sub => { self.emit(&[0x59, 0x58, 0x48, 0x29, 0xC8, 0x50]); }
                Op::Mul => { self.emit(&[0x59, 0x58, 0x48, 0x0F, 0xAF, 0xC1, 0x50]); }
                Op::Inc => {
                    let offset = (inst.arg + 1) * 8;
                    self.emit(&[0x48, 0xFF, 0x84, 0x24]); // inc [rsp + disp32]
                    self.emit_u32(offset);
                }
                Op::AddAssign => {
                    let offset = (inst.arg + 1) * 8;
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0x01, 0x84, 0x24]); // add [rsp + disp32], rax
                    self.emit_u32(offset);
                }
                Op::Lt => { self.emit_cmp(0x9C); }
                Op::Le => { self.emit_cmp(0x9E); }
                Op::Gt => { self.emit_cmp(0x9F); }
                Op::Eq => { self.emit_cmp(0x94); }
                Op::JumpIfFalse => {
                    self.emit(&[0x58, 0x48, 0x85, 0xC0, 0x0F, 0x84]);
                    self.jump_patches.push((self.code.len(), inst.arg as usize));
                    self.emit_u32(0);
                }
                Op::Loop => {
                    let target = i + 1 - inst.arg as usize;
                    self.emit(&[0xE9]);
                    self.jump_patches.push((self.code.len(), target));
                    self.emit_u32(0);
                }
                Op::Print => {
                    // Bare-metal VGA: write value to 0xB8000
                    self.emit(&[0x58]); // pop rax
                    // Convert int to ASCII digit + write to VGA at 0xB8000
                    self.emit(&[0x48, 0xBB]); // mov rbx, 0xB8000
                    self.emit_i64(0xB8000);
                    self.emit(&[0x04, 0x30]); // add al, '0' (simple digit)
                    self.emit(&[0x88, 0x03]); // mov [rbx], al
                    self.emit(&[0xC6, 0x43, 0x01, 0x0F]); // mov [rbx+1], 0x0F (white on black)
                }
                Op::Div => {
                    self.emit(&[0x59, 0x58]); // pop rcx, pop rax
                    self.emit(&[0x48, 0x99]); // cqo (sign extend rax to rdx:rax)
                    self.emit(&[0x48, 0xF7, 0xF9]); // idiv rcx
                    self.emit(&[0x50]); // push rax
                }
                Op::Mod => {
                    self.emit(&[0x59, 0x58]); // pop rcx, pop rax
                    self.emit(&[0x48, 0x99]); // cqo
                    self.emit(&[0x48, 0xF7, 0xF9]); // idiv rcx
                    self.emit(&[0x52]); // push rdx (remainder)
                }
                Op::Neg => {
                    self.emit(&[0x58]); // pop rax
                    self.emit(&[0x48, 0xF7, 0xD8]); // neg rax
                    self.emit(&[0x50]); // push rax
                }
                Op::Ne => { self.emit_cmp(0x95); }
                Op::Ge => { self.emit_cmp(0x9D); }
                Op::Jump => {
                    self.emit(&[0xE9]);
                    self.jump_patches.push((self.code.len(), inst.arg as usize));
                    self.emit_u32(0);
                }
                Op::Return => {
                    self.emit(&[0x58]); // pop rax (return value)
                    self.emit(&[0x48, 0x81, 0xC4]); // add rsp, aligned
                    self.emit_u32(aligned);
                    self.emit(&[0xC3]); // ret
                }
                Op::Halt => {
                    self.emit(&[0xF4]); // hlt
                }
                _ => { self.emit(&[0x90]); } // nop for unsupported
            }
        }
        self.emit(&[0xF4]); // hlt at end
        self.inst_offsets.push(self.code.len());

        for (patch_offset, target_inst) in &self.jump_patches {
            let target_offset = self.inst_offsets[*target_inst];
            let rel = (target_offset as i32) - (*patch_offset as i32 + 4);
            let bytes = rel.to_le_bytes();
            self.code[*patch_offset..(*patch_offset + 4)].copy_from_slice(&bytes);
        }

        self.code
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn emit_u32(&mut self, val: u32) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }

    fn emit_i64(&mut self, val: i64) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }

    // pop rcx; pop rax; cmp rax, rcx; setXX al; movzx rax, al; push rax
    fn emit_cmp(&mut self, setcc_opcode: u8) {
        self.emit(&[0x59, 0x58]); // pop rcx; pop rax
        self.emit(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
        self.emit(&[0x0F, setcc_opcode, 0xC0]); // setXX al
        self.emit(&[0x48, 0x0F, 0xB6, 0xC0]); // movzx rax, al
        self.emit(&[0x50]); // push rax
    }
}

// Executable memory region
pub struct JitFunction {
    ptr: *mut u8,
    size: usize,
}

extern "C" fn jit_print_i64(val: i64) {
    println!("{}", val);
}

#[cfg(target_os = "windows")]
impl JitFunction {
    pub fn new(code: Vec<u8>) -> Self {
        
        let size = code.len();
        let ptr = unsafe {
            windows_alloc_exec(size, &code)
        };
        Self { ptr, size }
    }

    pub fn execute(&self) -> i64 {
        let print_fn = jit_print_i64 as *const () as u64;
        let _func: extern "C" fn(u64) -> i64 = unsafe {
            std::mem::transmute(self.ptr)
        };
        // Pass print function pointer via r15 by wrapping in asm
        let result: i64;
        unsafe {
            std::arch::asm!(
                "mov r15, {print_fn}",
                "call {func}",
                print_fn = in(reg) print_fn,
                func = in(reg) self.ptr,
                out("rax") result,
                out("rcx") _,
                out("rdx") _,
                out("r15") _,
                clobber_abi("C"),
            );
        }
        result
    }

    pub fn execute_raw(&self) -> i64 {
        let func: extern "C" fn() -> i64 = unsafe { std::mem::transmute(self.ptr) };
        func()
    }

    pub fn execute_with_arg(&self, arg: i64) -> i64 {
        let func: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(self.ptr) };
        func(arg)
    }
}

#[cfg(target_os = "windows")]
impl Drop for JitFunction {
    fn drop(&mut self) {
        unsafe { windows_free_exec(self.ptr, self.size); }
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_alloc_exec(size: usize, code: &[u8]) -> *mut u8 {
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(addr: *mut u8, size: usize, alloc_type: u32, protect: u32) -> *mut u8;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    let ptr = VirtualAlloc(
        std::ptr::null_mut(),
        size,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    );
    if !ptr.is_null() {
        std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, size);
    }
    ptr
}

#[cfg(target_os = "windows")]
unsafe fn windows_free_exec(ptr: *mut u8, _size: usize) {
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualFree(addr: *mut u8, size: usize, free_type: u32) -> i32;
    }
    const MEM_RELEASE: u32 = 0x8000;
    VirtualFree(ptr, 0, MEM_RELEASE);
}

#[cfg(all(unix, target_arch = "x86_64"))]
impl JitFunction {
    pub fn new(code: Vec<u8>) -> Self {
        let size = code.len();
        let ptr = unsafe {
            let p = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1, 0,
            ) as *mut u8;
            std::ptr::copy_nonoverlapping(code.as_ptr(), p, size);
            p
        };
        Self { ptr, size }
    }

    pub fn execute(&self) -> i64 {
        let print_fn = jit_print_i64 as *const () as u64;
        let result: i64;
        unsafe {
            std::arch::asm!(
                "mov r15, {print_fn}",
                "call {func}",
                print_fn = in(reg) print_fn,
                func = in(reg) self.ptr,
                out("rax") result,
                out("rcx") _,
                out("rdx") _,
                out("r15") _,
                clobber_abi("C"),
            );
        }
        result
    }

    pub fn execute_raw(&self) -> i64 {
        let func: extern "C" fn() -> i64 = unsafe { std::mem::transmute(self.ptr) };
        func()
    }

    pub fn execute_with_arg(&self, arg: i64) -> i64 {
        let func: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(self.ptr) };
        func(arg)
    }
}

#[cfg(all(unix, target_arch = "x86_64"))]
impl Drop for JitFunction {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut _, self.size); }
    }
}
