/// # interpreter
///
/// (from: https://laurencescotford.com/chip-8-on-the-cosmac-vip-initialisation/)
/// RCA1802 has 16 16bit registers, each of which can be a program counter:
///  0. DMA pointer for screen refresh           -- ignore
///  1. interrupt program counter                -- ignore
///  2. stack pointer                            -- 0x6cf on 2k machine; 0xcf in penultimate page of RAM
///  3. interpreter subroutine program counter   -- this is the address of the decoded instruction's 1802 code
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
    instruction: Option<fn(&mut Chip8Interpreter<'a>) -> usize>,
    program_counter: u16,
    vx: u16,
    vy: u16,
    tone_timer: u8,
    general_timer: u8,
    random: u16,
    i: u16,
    display_pointer: u16,
    state: InterpreterState,
}

impl<'a> Chip8Interpreter<'a> {
    pub fn new(display: &mut impl display::Display) -> Result<Chip8Interpreter, io::Error> {
        let m = memory::Chip8MemoryMap::new()?;
        let mut i = Chip8Interpreter {
            memory: m,
            display,
            stack_pointer: 0x0000,
            instruction: None,
            program_counter: 0x0000,
            vx: 0x0000,
            vy: 0x0000,
            tone_timer: 0x00,
            general_timer: 0x00,
            random: 0x0000,
            i: 0x0000,
            display_pointer: 0x0000,
            state: InterpreterState::Start,
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

    /// TODO
    fn cycle(&mut self) -> usize {
        match self.state {
            InterpreterState::FetchDecode => self.fetch_and_decode(),
            InterpreterState::Execute => self.call(),
            _ => panic!("TODO"),
        }
    }

    pub fn main_loop(&mut self) {
        self.cycle();
    }

    /// fetch the instruction at the program counter, figure out what it is,
    /// update the program counter, update the interpreter state
    fn fetch_and_decode(&mut self) -> usize {
        let inst = self.memory.get_word(self.program_counter);

        self.instruction = Some(match inst {
            0x00e0 => Chip8Interpreter::inst_clear_screen,
            0xa000..=0xafff => Chip8Interpreter::inst_set_i,
            _ => panic!("Failed to decode instruction {:04x?}", inst),
        });

        self.program_counter += 2;
        self.state = InterpreterState::Execute;

        // execution time is 40 cycles for 0xxx and 68 cycles otherwise
        if inst > 0x0fff { 68 } else { 40 }
    }

    /// call the most recently-decoded instruction
    fn call(&mut self) -> usize {
        self.state = InterpreterState::FetchDecode;
        match self.instruction {
            Some(i) => i(self),
            None => panic!("Null pointer exception?!")
        }
    }

    /// 00e0
    fn inst_clear_screen(&mut self) -> usize {
        1
    }

    /// annn
    fn inst_set_i(&mut self) -> usize {
        2
    }
}

/// state machine for fetch-decode-execute-interrupt. it's in the state before
/// and during it's doing the thing. so think "fetch-ing", "ready to fetch", ...
///
/// |                  .-----------------------.
/// |                  v                       |
/// | .-------.    .----------------.     .---------.
/// | | start |--->| fetch + decode |---->| execute |
/// | `-------'    `----------------'     `---------'
/// |                  ^                       |
/// |                  |   .---------------.   |
/// |                  `---| interruptable |<--'
/// |                      `---------------'
#[derive(PartialEq)]
enum InterpreterState {
    Start,
    FetchDecode,
    Execute,
    Interrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_with(f: fn(i: &mut Chip8Interpreter) -> Result<(), io::Error>) -> Result<(), io::Error> {
        let mut display = display::DummyDisplay::new()?;
        let mut i = Chip8Interpreter::new(&mut display)?;
        let mut prog: &[u8] = &[0x00, 0xe0, 0xa2, 0x2a, 0x60, 0x0c];
        i.load_program(&mut prog)?;
        f(&mut i)
    }

    #[test]
    fn test_program_load_ok() -> Result<(), io::Error> {
        test_with(|_i| { Ok(()) })
    }

    #[test]
    fn test_fetch_and_decode_moves_pc() -> Result<(), io::Error> {
        test_with(|i| {
            let _ = i.fetch_and_decode();
            assert_eq!(i.program_counter, 0x202);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_sets_state() -> Result<(), io::Error> {
        test_with(|i| {
            let _ = i.fetch_and_decode();
            assert!(i.state == InterpreterState::Execute);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_zero_inst_duration() -> Result<(), io::Error> {
        // 0xxx instructions take 40 machine cycles on the original chip-8
        // the first test fixture instruction is 00e0
        test_with(|i| {
            assert_eq!(i.fetch_and_decode(), 40);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_other_inst_duration() -> Result<(), io::Error> {
        // other instructions take 68 machine cycles
        // the second test fixture instruction is axxx
        test_with(|i| {
            let _ = i.fetch_and_decode();
            assert_eq!(i.fetch_and_decode(), 68);
            Ok(())
        })
    }

    #[test]
    fn test_call_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let _ = i.fetch_and_decode();
            assert_eq!(i.call(), 1);
            Ok(())
        })
    }

}
