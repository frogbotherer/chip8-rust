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
    // contains the decoded instruction and the original four bytes
    // TODO use an enum or struct instead of Option?
    instruction: Option<fn(&mut Chip8Interpreter<'a>) -> Result<usize, io::Error>>,
    instruction_data: u16,
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
            instruction_data: 0x0000,
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
        // TODO soft-code
        self.display
            .draw(self.memory.get_ro_slice(self.display_pointer, 0x100))
    }

    /// TODO
    fn cycle(&mut self) -> Result<usize, io::Error> {
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
    /// set vx/vy, update the program counter, update the interpreter state
    fn fetch_and_decode(&mut self) -> Result<usize, io::Error> {
        let inst = self.memory.get_word(self.program_counter);

        // first byte, second nybble
        self.vx = (inst & 0x0f00) >> 8;
        // second byte, first nybble
        self.vy = (inst & 0x00f0) >> 4;

        self.instruction = Some(match inst {
            0x00e0 => Chip8Interpreter::inst_clear_screen,
            0x1000..=0x1fff => Chip8Interpreter::inst_branch,
            0x6000..=0x6fff => Chip8Interpreter::inst_load_vx,
            0x7000..=0x7fff => Chip8Interpreter::inst_add_to_vx,
            0xa000..=0xafff => Chip8Interpreter::inst_set_i,
            _ => panic!("Failed to decode instruction {:04x?}", inst),
        });

        self.instruction_data = inst;

        self.program_counter += 2;
        self.state = InterpreterState::Execute;

        // execution time is 40 cycles for 0xxx and 68 cycles otherwise
        if inst > 0x0fff { Ok(68) } else { Ok(40) }
    }

    /// call the most recently-decoded instruction
    fn call(&mut self) -> Result<usize, io::Error> {
        self.state = InterpreterState::FetchDecode;
        match self.instruction {
            Some(i) => i(self),
            None => panic!("Null pointer exception?!")
        }
    }

    /// 00e0
    fn inst_clear_screen(&mut self) -> Result<usize, io::Error> {
        // TODO: soft-code
        self.memory.write(&[0; 0x0100], self.display_pointer, 0x0100)?;
        Ok(24)
    }
    /// 1nnn
    fn inst_branch(&mut self) -> Result<usize, io::Error> {
        self.program_counter = self.instruction_data & 0xfff;
        Ok(12)
    }
    /// 6xnn
    fn inst_load_vx(&mut self) -> Result<usize, io::Error> {
        self.memory.write(&[(self.instruction_data & 0xff) as u8], self.memory.var_addr + self.vx, 1)?;
        Ok(6)
    }
    /// 7xnn
    fn inst_add_to_vx(&mut self) -> Result<usize, io::Error> {
        let v = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        v[0] = (((v[0] as u16) + (self.instruction_data & 0xff)) & 0xff) as u8;
        Ok(10)
    }
    /// annn
    // see https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
    fn inst_set_i(&mut self) -> Result<usize, io::Error> {
        self.i = self.instruction_data & 0xfff;
        Ok(12)
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
    WaitInterrupt // waiting for an interrupt
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
            let _ = i.fetch_and_decode()?;
            assert_eq!(i.program_counter, 0x202);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_sets_state() -> Result<(), io::Error> {
        test_with(|i| {
            let _ = i.fetch_and_decode()?;
            assert!(i.state == InterpreterState::Execute);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_zero_inst_duration() -> Result<(), io::Error> {
        // 0xxx instructions take 40 machine cycles on the original chip-8
        // the first test fixture instruction is 00e0
        test_with(|i| {
            assert_eq!(i.fetch_and_decode()?, 40);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_other_inst_duration() -> Result<(), io::Error> {
        // other instructions take 68 machine cycles
        // the second test fixture instruction is axxx
        test_with(|i| {
            let _ = i.fetch_and_decode()?;
            assert_eq!(i.fetch_and_decode()?, 68);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_sets_vx() -> Result<(), io::Error> {
        test_with(|i| {
            // second test fixture instruction is a22a
            let _ = i.fetch_and_decode()?;
            let _ = i.fetch_and_decode()?;
            assert_eq!(i.vx, 0x02);
            Ok(())
        })
    }

    #[test]
    fn test_fetch_and_decode_sets_vy() -> Result<(), io::Error> {
        test_with(|i| {
            // first test fixture instruction is 0e00
            let _ = i.fetch_and_decode()?;
            assert_eq!(i.vy, 0x0e);
            Ok(())
        })
    }

    #[test]
    fn test_call_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let _ = i.fetch_and_decode()?;
            assert_eq!(i.call()?, 24);  // cycles for 0e00
            Ok(())
        })
    }

    #[test]
    fn test_clear_screen() -> Result<(), io::Error> {
        // 0e00
        test_with(|i| {
            // fill display memory with 1s
            let m: &[u8] = &[1; 256];
            i.memory.write(&m, 0xf00, 0x100)?;

            // call 0e00
            let _ = i.fetch_and_decode()?;
            let t = i.inst_clear_screen()?;

            assert_eq!(i.memory.get_ro_slice(0xf00, 0x100), &[0; 256]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-machine-code-integration/
            // takes 24 cycles
            assert_eq!(t, 24);
            Ok(())
        })
    }

    #[test]
    fn test_branch() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x12, 0x34];
            i.load_program(&mut m)?;

            // call 1234
            let _ = i.fetch_and_decode()?;
            let t = i.inst_branch()?;

            assert_eq!(i.program_counter, 0x234);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 12 cycles
            assert_eq!(t, 12);
            Ok(())
        })
    }

    #[test]
    fn test_load_vx() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x61, 0x23];
            i.load_program(&mut m)?;

            // call 6123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_load_vx()?;

            assert_eq!(i.vx, 1);
            // 0xef0 is where vx variables are on 4k layout
            assert_eq!(i.memory.get_ro_slice(0xef0, 16), &[0, 0x23, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 6 cycles
            assert_eq!(t, 6);
            Ok(())
        })
    }

    #[test]
    fn test_add_to_vx() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x71, 0x99];
            i.load_program(&mut m)?;

            // call 7123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_add_to_vx()?;

            assert_eq!(i.vx, 1);
            // 0xef0 is where vx variables are on 4k layout
            assert_eq!(i.memory.get_ro_slice(0xef0, 16), &[0, 0x99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-arithmetic-and-logic-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
            Ok(())
        })
    }

    #[test]
    fn test_add_to_vx_overrun() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x61, 0x81, 0x71, 0x82];
            i.load_program(&mut m)?;

            // call 7123
            let _ = i.fetch_and_decode()?;
            let _ = i.inst_load_vx()?;
            let _ = i.fetch_and_decode()?;
            let _ = i.inst_add_to_vx()?;

            // 0xef0 is where vx variables are on 4k layout
            assert_eq!(i.memory.get_ro_slice(0xef0, 16), &[0, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

            Ok(())
        })
    }

    #[test]
    fn test_set_i() -> Result<(), io::Error> {
        // annn
        test_with(|i| {
            let mut m: &[u8] = &[0xa1, 0x23];
            i.load_program(&mut m)?;

            // call a123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_set_i()?;

            assert_eq!(i.i, 0x123);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 12 cycles
            assert_eq!(t, 12);
            Ok(())
        })
    }

}
