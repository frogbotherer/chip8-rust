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
use std::{io, thread, time};

const CHIP8_TARGET_FREQ_NS: u64 = 1_000_000_000 / 60; // 60 fps
const CHIP8_CYCLE_NS: u64 = 4540; // 4.54 us

pub struct Chip8Interpreter<'a> {
    memory: memory::Chip8MemoryMap,
    display: &'a mut dyn display::Display,
    stack_pointer: u16,
    // contains the decoded instruction and the original four bytes
    // TODO use an enum or struct instead of Option?
    instruction: Option<fn(&mut Chip8Interpreter<'a>) -> Result<usize, io::Error>>,
    pub instruction_data: u16,
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
            state: InterpreterState::FetchDecode,
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
    pub fn interrupt(&mut self) -> Result<usize, io::Error> {
        // TODO soft-code
        self.display
            .draw(self.memory.get_ro_slice(self.display_pointer, 0x100))?;

        // if we'd been waiting for an interrupt, put the interpreter back into
        // the Execute state, because it will have been mid-instruction
        if self.state == InterpreterState::WaitInterrupt {
            self.state = InterpreterState::Execute;
        }
        // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-interrupts/
        Ok(807 + 1024)
    }

    /// step the interpreter forward one state, returning number of machine
    /// cycles consumed.
    fn cycle(&mut self) -> Result<usize, io::Error> {
        match self.state {
            InterpreterState::FetchDecode => self.fetch_and_decode(),
            InterpreterState::Execute => self.call(),
            InterpreterState::WaitInterrupt => Ok(1),
        }
    }

    /// run the main interpreter loop, including timing and interrupts
    pub fn main_loop(&mut self, frame_count: usize) -> Result<(), io::Error> {
        let mut remaining_sleep = time::Duration::from_nanos(0);

        // loop of frames
        for frame in 0..frame_count {
            // |c......................................................|
            //  ^-now                                                  ^-frame end
            let mut now = time::Instant::now();
            let frame_end = now + time::Duration::from_nanos(CHIP8_TARGET_FREQ_NS);

            // interrupt at the top of the loop, so that the time spent in the
            // isr is inside the frame (rather than frame.time->isr.time->frame.time->etc.)
            let t = self.interrupt()?;

            // how long we should sleep for, for the interrupt
            let inst_end =
                now + time::Duration::from_nanos(CHIP8_CYCLE_NS * t as u64) + remaining_sleep;
            now = time::Instant::now();
            // |..c.....|..............................................|
            //    ^-now ^-inst_end                                     ^-frame end

            if inst_end >= now {
                thread::sleep(inst_end - now);
            } else {
                eprintln!(
                    "{:09?}: Warning: ISR took longer than COSMAC by {:?}",
                    frame,
                    now - inst_end
                );
            }
            // |........|c.............................................|
            //    ^-now ^-inst_end                                     ^-frame end

            // loop of instructions within each frame
            loop {
                now = time::Instant::now();
                let t = self.cycle()?;
                // |........|..c...........................................|
                //           ^-now                                         ^-frame end

                // how long we should sleep until
                let inst_end = now + time::Duration::from_nanos(CHIP8_CYCLE_NS * t as u64);
                now = time::Instant::now();
                // |........|..c.....|.....................................|
                //             ^-now ^-inst_end                            ^-frame end

                // if we would sleep past the end of the frame, store the
                // remainder and interrupt
                if inst_end >= frame_end {
                    remaining_sleep = inst_end - frame_end;
                    // we can legitimately overrun the end of the frame during the instruction
                    if frame_end >= now {
                        thread::sleep(frame_end - now);
                    }
                    break;
                } else {
                    if inst_end >= now {
                        thread::sleep(inst_end - now);
                    } else {
                        eprintln!(
                            "{:09?}: Warning: {:04x?} took longer than COSMAC by {:?}",
                            frame,
                            self.instruction_data,
                            now - inst_end
                        );
                    }
                }
            }
        }
        Ok(())
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
            0x00ee => Chip8Interpreter::inst_ret,
            0x1000..=0x1fff => Chip8Interpreter::inst_branch,
            0x2000..=0x2fff => Chip8Interpreter::inst_subroutine,
            0x6000..=0x6fff => Chip8Interpreter::inst_load_vx,
            0x7000..=0x7fff => Chip8Interpreter::inst_add_to_vx,
            0xa000..=0xafff => Chip8Interpreter::inst_set_i,
            0xb000..=0xbfff => Chip8Interpreter::inst_jump_with_offset,
            0xd000..=0xdfff => Chip8Interpreter::inst_draw_sprite,
            _ => panic!("Failed to decode instruction {:04x?}", inst),
        });

        self.instruction_data = inst;

        self.program_counter += 2;
        self.state = InterpreterState::Execute;

        // execution time is 40 cycles for 0xxx and 68 cycles otherwise
        if inst > 0x0fff {
            Ok(68)
        } else {
            Ok(40)
        }
    }

    /// call the most recently-decoded instruction
    fn call(&mut self) -> Result<usize, io::Error> {
        // NB. ordering is important here because instructions can (and need
        //     to) modify the interpreter state
        self.state = InterpreterState::FetchDecode;
        match self.instruction {
            Some(i) => i(self),
            None => panic!("Null pointer exception?!"),
        }
    }

    /// 00e0
    fn inst_clear_screen(&mut self) -> Result<usize, io::Error> {
        // TODO: soft-code
        self.memory
            .write(&[0; 0x0100], self.display_pointer, 0x0100)?;
        Ok(24)
    }

    /// 00ee
    fn inst_ret(&mut self) -> Result<usize, io::Error> {
        self.stack_pointer += 2;
        self.program_counter = self.memory.get_word(self.stack_pointer);
        Ok(10)
    }

    /// 1nnn
    fn inst_branch(&mut self) -> Result<usize, io::Error> {
        self.program_counter = self.instruction_data & 0xfff;
        Ok(12)
    }

    /// 2nnn
    fn inst_subroutine(&mut self) -> Result<usize, io::Error> {
        self.memory.write(
            &[
                (self.program_counter >> 8) as u8,
                (self.program_counter & 0xff) as u8,
            ],
            self.stack_pointer,
            2,
        )?;
        self.stack_pointer -= 2;
        self.program_counter = self.instruction_data & 0xfff;
        Ok(26)
    }

    /// 6xnn
    fn inst_load_vx(&mut self) -> Result<usize, io::Error> {
        self.memory.write(
            &[(self.instruction_data & 0xff) as u8],
            self.memory.var_addr + self.vx,
            1,
        )?;
        Ok(6)
    }

    /// 7xnn
    fn inst_add_to_vx(&mut self) -> Result<usize, io::Error> {
        let v = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        v[0] = (((v[0] as u16) + (self.instruction_data & 0xff)) & 0xff) as u8;
        Ok(10)
    }

    /// bnnn
    fn inst_jump_with_offset(&mut self) -> Result<usize, io::Error> {
        // TODO CHIP-48 and SUPERCHIP variants
        let offset = self.memory.get_ro_slice(self.memory.var_addr, 1)[0] as u16; // add self.vx for variations
        self.program_counter = (self.instruction_data & 0xfff) + offset;
        if self.instruction_data & 0xf00 != self.program_counter & 0xf00 {
            // crosses a page boundary
            Ok(24)
        } else {
            Ok(22)
        }
    }

    /// dxyn
    fn inst_draw_sprite(&mut self) -> Result<usize, io::Error> {
        //
        //  x_bit_offset
        // -->|                       (work ram contents)
        //    .xxxxx...  |            ....xxxx x.......
        //    x.....x..  |            ...x.... .x......
        //    x.x.x.x..  | rows  ==>  ...x.x.x .x......
        //    x.....x..  v            ...x.... .x......
        //    .x.x.x...  -            ....x.x. x.......
        //
        // bit offset from byte margin
        let x_bit_offset = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0] & 0x7;

        // number of rows in the sprite
        let rows = self.instruction_data & 0xf;

        // data to draw (copied to a vec to avoid shenanigans with borrowing)
        let sprite = self.memory.get_ro_slice(self.i, rows as usize).to_vec();

        // writable work area
        let work = self.memory.get_rw_slice(self.memory.work_addr, 32);

        // write a correctly left-shifted version of the sprite into the work area
        for (idx, byte) in sprite.iter().enumerate() {
            work[idx * 2] = byte >> x_bit_offset;
            work[idx * 2 + 1] = if x_bit_offset == 0 {
                0x0
            } else {
                byte << (8 - x_bit_offset)
            };
        }

        // wait for the next display interrupt
        self.state = InterpreterState::WaitInterrupt;
        self.instruction = Some(Chip8Interpreter::inst_draw_sprite_pt2);

        // duration is [ROUGHLY!]
        //     25 for preamble
        //   + 10 * (rows * x_bit_offset) for instructions for offsetting
        //   + 7 * (rows) for each row
        //   + 1 for the interrupt wait instruction
        Ok((26 + 10 * rows * (x_bit_offset as u16) + 7 * rows) as usize)
    }

    /// dxyn (after the interrupt)
    fn inst_draw_sprite_pt2(&mut self) -> Result<usize, io::Error> {
        let mut dur = 12;

        // display x and y coords (in bits) (again)
        // TODO these are hard-wired to CHIP-8 display dimensions
        let vx_val = 0x3f & self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0] as usize;
        let vy_val = 0x1f & self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0] as usize;

        // address to start drawing sprite in memory
        let draw_addr = vx_val / 8 // x byte offset
                      + vy_val * 8; // y byte offset

        // readable work area
        let work = self.memory.get_ro_slice(self.memory.work_addr, 32).to_vec();

        // writable vram
        // TODO soft-code size
        let vram = self.memory.get_rw_slice(self.memory.display_addr, 0x100);

        // collision flag (gets written to VF when done)
        let mut collision_flag: u8 = 0;

        // iterate thru pairs of bytes, looking for collisions and whether (for
        // the right-hand byte) they can be displayed or not.
        for (idx, byte) in work.iter().enumerate() {
            // TODO [again] this 8-byte stride is hard-coded to the width of the screen
            let this_addr = draw_addr + (idx / 2) * 0x8 + idx % 2;
            if this_addr > vram.len() {
                // drawing off the bottom of the screen
                continue;
            }
            if idx % 2 == 1 && (this_addr & 0x3f) == 0 {
                // TODO and this
                // right-hand byte hangs off the edge of the screen
                continue;
            }
            if vram[this_addr] & *byte != *byte {
                collision_flag = 1;
                dur += 2;
            }
            vram[this_addr] ^= byte;
            dur += if idx % 2 == 0 { 17 } else { 8 }
        }

        // save the collision flag in VF
        self.memory
            .write(&[collision_flag], self.memory.var_addr + 0xf, 1)?;

        // duration is:
        //    (6+6) for preamble/postamble
        //  + (6+6+5) * rows for left byte
        //  + 2 * rows for lbyte collision
        //  + (4 + 4) * rows for right byte (if visible)
        //  + 2 * rows for rbyte collision
        Ok(dur)
    }

    /// annn
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
    FetchDecode,
    Execute,
    WaitInterrupt, // waiting for an interrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_with(
        f: fn(i: &mut Chip8Interpreter) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        let mut display = display::DummyDisplay::new()?;
        let mut i = Chip8Interpreter::new(&mut display)?;
        let mut prog: &[u8] = &[0x00, 0xe0, 0xa2, 0x2a, 0x60, 0x0c];
        i.load_program(&mut prog)?;
        f(&mut i)
    }

    #[test]
    fn test_program_load_ok() -> Result<(), io::Error> {
        test_with(|_i| Ok(()))
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
            assert_eq!(i.call()?, 24); // cycles for 0e00
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
    fn test_subroutine() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x23, 0x45];
            i.load_program(&mut m)?;

            // call 2345
            let _ = i.fetch_and_decode()?;
            let t = i.inst_subroutine()?;

            assert_eq!(i.memory.get_ro_slice(0xece, 2), &[0x02, 0x02]);
            assert_eq!(i.stack_pointer, 0xecc);
            assert_eq!(i.program_counter, 0x345);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 26 cycles
            assert_eq!(t, 26);
            Ok(())
        })
    }

    #[test]
    fn test_ret() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x22, 0x04, 0x00, 0xe0, 0x00, 0xee];
            i.load_program(&mut m)?;

            // call 2345
            let _ = i.fetch_and_decode()?;
            let _ = i.call()?;
            let _ = i.fetch_and_decode()?;
            let t = i.inst_ret()?;

            assert_eq!(i.memory.get_ro_slice(0xece, 2), &[0x02, 0x02]);
            assert_eq!(i.stack_pointer, 0xece);
            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
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
            assert_eq!(
                i.memory.get_ro_slice(0xef0, 16),
                &[0, 0x23, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            );

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
            assert_eq!(
                i.memory.get_ro_slice(0xef0, 16),
                &[0, 0x99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            );

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
            assert_eq!(
                i.memory.get_ro_slice(0xef0, 16),
                &[0, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            );

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

    #[test]
    fn test_jump_offset() -> Result<(), io::Error> {
        // bnnn
        test_with(|i| {
            let mut m: &[u8] = &[0xb1, 0x23];
            i.load_program(&mut m)?;
            i.memory.write(&[0x40], 0xef0, 1)?;

            // call b123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_jump_with_offset()?;

            assert_eq!(i.program_counter, 0x163);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 22 cycles within a page
            assert_eq!(t, 22);
            Ok(())
        })
    }

    #[test]
    fn test_jump_offset_across_pages() -> Result<(), io::Error> {
        // bnnn
        test_with(|i| {
            let mut m: &[u8] = &[0xb1, 0x23];
            i.load_program(&mut m)?;
            i.memory.write(&[0xdd], 0xef0, 1)?;

            // call b123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_jump_with_offset()?;

            assert_eq!(i.program_counter, 0x200);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 24 cycles across pages
            assert_eq!(t, 24);
            Ok(())
        })
    }

    #[test]
    fn test_dxyn_waits() -> Result<(), io::Error> {
        // dxyn
        test_with(|i| {
            let mut m: &[u8] = &[
                0xa2, 0x06, 0x60, 0x04, 0xd0, 0x05, 0xf0, 0x78, 0x3c, 0x1e, 0x0f,
            ];
            i.load_program(&mut m)?;

            // call d008
            for _ in 0..6 {
                i.cycle()?;
            }
            let t = i.inst_draw_sprite()?;

            assert!(i.state == InterpreterState::WaitInterrupt);
            assert_eq!(i.instruction_data, 0xd005);
            //assert_eq!(i.instruction, Some(Chip8Interpreter::inst_draw_sprite_pt2));
            //
            // xxxx....      ....xxxx ........
            // .xxxx...      .....xxx x.......
            // ..xxxx..  ==> ......xx xx......
            // ...xxxx.      .......x xxx.....
            // ....xxxx      ........ xxxx....
            assert_eq!(
                i.memory.get_ro_slice(0xed0, 32),
                &[
                    0x0f, 0x00, 0x07, 0x80, 0x03, 0xc0, 0x01, 0xe0, 0x00, 0xf0, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00
                ]
            );

            assert_eq!(t, 261);
            Ok(())
        })
    }

    #[test]
    fn test_dxyn_pt2() -> Result<(), io::Error> {
        // dxyn
        test_with(|i| {
            let mut m: &[u8] = &[
                0xa2, 0x06, 0x60, 0x04, 0xd0, 0x05, 0xf0, 0x78, 0x3c, 0x1e, 0x0f,
            ];
            i.load_program(&mut m)?;

            // write a colliding px into vram to test collision bit
            i.memory.write(&[0x08], 0xf20, 1)?;

            // call d008
            for _ in 0..7 {
                i.cycle()?;
            }
            let t = i.inst_draw_sprite_pt2()?;

            assert_eq!(
                // 5 rows of vram across where the sprite should be
                i.memory.get_ro_slice(0xf20, 0x28),
                &[
                    0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x80, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x03, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xe0,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00
                ]
            );

            // vf == 1
            assert_eq!(i.memory.get_ro_slice(0xeff, 1)[0], 1);

            assert_eq!(t, 428);
            Ok(())
        })
    }
}
