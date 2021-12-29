/// # interpreter
///
/// (from: https://laurencescotford.com/chip-8-on-the-cosmac-vip-initialisation/)
/// RCA1802 has 16 16bit registers, each of which can be a program counter:
///  0. DMA pointer for screen refresh           -- ignore
///  1. interrupt program counter                -- ignore
///  2. stack pointer                            -- 0x6cf on 2k machine; 0xcf in penultimate page of RAM
///  3. interpreter subroutine program counter   -- ignore
///  4. CALL subroutine program counter          -- ignore (this is for the interpreter's own fetch/decode)
///  5. chip-8 program counter                   -- 0x200
///  6. VX pointer
///  7. VY pointer
///  8.0 (low bits) tone timer
///  8.1 (high bits) general timer
///  9. random number
///  A. I pointer
///  B. display page pointer                     -- 0x700 on 2k machine; last page of RAM
///  C-F. temporary storage                      -- ignore
/// it also has:
///  P (4bit register) for determining which of R0-F is the current PC
///  X (4bit register) for "           "     "  R0-F is a pointer to a RAM address
/// ... yes P and X can be set to the same register. yes we can ignore them.
use crate::{display, memory, memory::MemoryMap};
use std::io;

pub struct Chip8Interpreter<'a> {
    memory: memory::Chip8MemoryMap,
    display: &'a mut dyn display::Display,
    stack_pointer: u16,
    program_counter: u16,
    vx: u16,
    vy: u16,
    tone_timer: u8,
    general_timer: u8,
    random: u16,
    i: u16,
    display_pointer: u16,
}

impl<'a> Chip8Interpreter<'a> {
    pub fn new(display: &mut impl display::Display) -> Result<Chip8Interpreter, io::Error> {
        let m = memory::Chip8MemoryMap::new()?;
        let mut i = Chip8Interpreter {
            memory: m,
            display,
            stack_pointer: 0x0000,
            program_counter: 0x0000,
            vx: 0x0000,
            vy: 0x0000,
            tone_timer: 0x00,
            general_timer: 0x00,
            random: 0x0000,
            i: 0x0000,
            display_pointer: 0x0000,
        };
        i.stack_pointer = i.memory.stack_addr;
        i.program_counter = i.memory.program_addr;
        i.display_pointer = i.memory.display_addr;
        Ok(i)
    }

    /// load a chip8 program
    pub fn load_program(&mut self, reader: &mut impl io::Read) -> Result<(), io::Error> {
        self.memory.load_program(reader)
    }

    /// external interrupt
    pub fn interrupt(&mut self) -> Result<(), io::Error> {
        self.display
            .draw(self.memory.get_ro_slice(self.display_pointer, 0x100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_program_load_ok() -> Result<(), io::Error> {
        let mut display = display::MonoTermDisplay::new(64, 32)?;
        let mut i = Chip8Interpreter::new(&mut display)?;
        let mut prog: &[u8] = &[0x00, 0xe0]; // clear screen
        i.load_program(&mut prog)
    }
}
