// Тризуб Native Compiler — AST → flat x86_64 binary
// Генерує standalone бінарник без runtime залежностей

use super::compiler::Compiler;
use super::jit::JitCompiler;

pub struct NativeCompiler;

impl NativeCompiler {
    pub fn compile_to_flat_binary(source: &str, output: &str) -> anyhow::Result<()> {
        let tokens = tryzub_lexer::tokenize(source)?;
        let ast = tryzub_parser::parse(tokens)?;

        let compiler = Compiler::new();
        let chunk = compiler.compile_program(&ast);

        let jit = JitCompiler::new();
        let machine_code = jit.compile_to_bytes(&chunk);

        std::fs::write(output, &machine_code)?;
        Ok(())
    }

    pub fn compile_to_bootable(source: &str, output: &str) -> anyhow::Result<()> {
        let tokens = tryzub_lexer::tokenize(source)?;
        let ast = tryzub_parser::parse(tokens)?;

        let compiler = Compiler::new();
        let chunk = compiler.compile_program(&ast);

        let jit = JitCompiler::new();
        let kernel_code = jit.compile_to_bytes(&chunk);

        // Мінімальний boot sector (512 байт) → переходить на ядро
        let mut boot: Vec<u8> = Vec::with_capacity(512);

        // CLI + A20 line
        boot.extend_from_slice(&[0xFA]); // cli
        boot.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax
        boot.extend_from_slice(&[0x8E, 0xD8]); // mov ds, ax
        boot.extend_from_slice(&[0x8E, 0xC0]); // mov es, ax
        boot.extend_from_slice(&[0x8E, 0xD0]); // mov ss, ax
        boot.extend_from_slice(&[0xBC, 0x00, 0x7C]); // mov sp, 0x7C00

        // Завантажити ядро з диска (LBA 1) на 0x10000
        // INT 13h AH=02h: читати сектори
        let kernel_sectors = ((kernel_code.len() + 511) / 512) as u8;
        boot.extend_from_slice(&[0xB4, 0x02]); // mov ah, 02h (read sectors)
        boot.push(0xB0); boot.push(kernel_sectors.max(1)); // mov al, N sectors
        boot.extend_from_slice(&[0xB5, 0x00]); // mov ch, 0 (cylinder)
        boot.extend_from_slice(&[0xB6, 0x00]); // mov dh, 0 (head)
        boot.extend_from_slice(&[0xB1, 0x02]); // mov cl, 2 (sector 2, 1-indexed)
        boot.extend_from_slice(&[0xBB, 0x00, 0x00]); // mov bx, 0x0000
        boot.extend_from_slice(&[0xB8, 0x00, 0x10]); // mov ax, 0x1000
        boot.extend_from_slice(&[0x8E, 0xC0]); // mov es, ax
        boot.extend_from_slice(&[0xCD, 0x13]); // int 13h

        // Перехід в protected mode
        // lgdt [gdt_ptr]
        let gdt_offset = 0x50u8; // offset within boot sector
        boot.extend_from_slice(&[0x0F, 0x01, 0x16]); // lgdt [gdt_ptr]
        boot.push(gdt_offset); boot.push(0x7C); // address: 0x7C00 + offset

        // Set PE bit in CR0
        boot.extend_from_slice(&[0x0F, 0x20, 0xC0]); // mov eax, cr0
        boot.extend_from_slice(&[0x66, 0x83, 0xC8, 0x01]); // or eax, 1
        boot.extend_from_slice(&[0x0F, 0x22, 0xC0]); // mov cr0, eax

        // Far jump to 32-bit code
        boot.extend_from_slice(&[0x66, 0xEA]); // jmp far 0x08:addr
        let jump_target = 0x7C00u32 + boot.len() as u32 + 6;
        boot.extend_from_slice(&jump_target.to_le_bytes());
        boot.extend_from_slice(&[0x08, 0x00]); // segment selector

        // 32-bit protected mode code
        boot.extend_from_slice(&[0x66, 0xB8, 0x10, 0x00]); // mov ax, 0x10
        boot.extend_from_slice(&[0x8E, 0xD8]); // mov ds, ax
        boot.extend_from_slice(&[0x8E, 0xD0]); // mov ss, ax
        boot.extend_from_slice(&[0x8E, 0xC0]); // mov es, ax

        // Jump to kernel at 0x10000
        boot.extend_from_slice(&[0x66, 0xEA]); // jmp far
        boot.extend_from_slice(&0x10000u32.to_le_bytes());
        boot.extend_from_slice(&[0x08, 0x00]);

        // GDT at offset 0x50
        while boot.len() < gdt_offset as usize {
            boot.push(0x90); // nop padding
        }

        // Null descriptor
        boot.extend_from_slice(&[0x00; 8]);
        // Code segment: base=0, limit=4GB, 32-bit, execute/read
        boot.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xCF, 0x00]);
        // Data segment: base=0, limit=4GB, 32-bit, read/write
        boot.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0xCF, 0x00]);
        // GDT pointer
        let gdt_limit = 23u16;
        let gdt_base = 0x7C00u32 + gdt_offset as u32;
        boot.extend_from_slice(&gdt_limit.to_le_bytes());
        boot.extend_from_slice(&gdt_base.to_le_bytes());

        // Pad to 510, add boot signature
        while boot.len() < 510 {
            boot.push(0x00);
        }
        boot.push(0x55);
        boot.push(0xAA);

        // Append kernel code padded to sector boundary
        let mut disk_image = boot;
        disk_image.extend_from_slice(&kernel_code);
        while disk_image.len() % 512 != 0 {
            disk_image.push(0x00);
        }

        std::fs::write(output, &disk_image)?;
        Ok(())
    }
}
