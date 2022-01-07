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
use crate::{display, input, memory, memory::MemoryMap};
use std::{io, thread, time};

const CHIP8_TARGET_FREQ_NS: u64 = 1_000_000_000 / 60; // 60 fps
const CHIP8_CYCLE_NS: u64 = 4540; // 4.54 us

pub struct Chip8Interpreter<'a> {
    memory: memory::Chip8MemoryMap,
    display: &'a mut dyn display::Display,
    input: &'a mut dyn input::Input,
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
    pub fn new(display: &'a mut impl display::Display, input: &'a mut impl input::Input) -> Result<Chip8Interpreter<'a>, io::Error> {
        let m = memory::Chip8MemoryMap::new()?;
        let mut i = Chip8Interpreter {
            memory: m,
            display,
            input,
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
    fn interrupt(&mut self) -> Result<usize, io::Error> {
        // duration
        // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-interrupts/
        let mut dur = 807 + 1024;

        // increment random seed
        self.random = self.random.wrapping_add(1);

        // update general timer
        if self.general_timer > 0 {
            self.general_timer -= 1;
            dur += 8;
        }

        // update tone timer
        if self.tone_timer > 0 {
            self.tone_timer -= 1;
            dur += 4;
        }

        // TODO soft-code size
        self.display
            .draw(self.memory.get_ro_slice(self.display_pointer, 0x100))?;

        // if we'd been waiting for an interrupt, put the interpreter back into
        // the Execute state, because it will have been mid-instruction
        if self.state == InterpreterState::WaitInterrupt {
            self.state = InterpreterState::Execute;
        }
        Ok(dur)
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
            0x3000..=0x3fff => Chip8Interpreter::inst_skip_vx_eq,
            0x4000..=0x4fff => Chip8Interpreter::inst_skip_vx_ne,
            0x5000..=0x5fff => Chip8Interpreter::inst_x_eq_y,
            0x6000..=0x6fff => Chip8Interpreter::inst_load_vx,
            0x7000..=0x7fff => Chip8Interpreter::inst_add_to_vx,
            0x8000..=0x8fff => match inst & 0xf {
                0x0 => Chip8Interpreter::inst_load_x_with_y,
                0x1 => Chip8Interpreter::inst_x_or_with_y,
                0x2 => Chip8Interpreter::inst_x_and_with_y,
                0x3 => Chip8Interpreter::inst_x_xor_with_y,
                0x4 => Chip8Interpreter::inst_x_add_y,
                0x5 => Chip8Interpreter::inst_x_minus_y,
                0x6 => Chip8Interpreter::inst_rshift_y_load_x,
                0x7 => Chip8Interpreter::inst_y_minus_x,
                0xe => Chip8Interpreter::inst_lshift_y_load_x,
                _ => panic!("Failed to decode instruction {:04x?}", inst),
            },
            0x9000..=0x9fff => Chip8Interpreter::inst_x_ne_y,
            0xa000..=0xafff => Chip8Interpreter::inst_set_i,
            0xb000..=0xbfff => Chip8Interpreter::inst_jump_with_offset,
            0xc000..=0xcfff => Chip8Interpreter::inst_random,
            0xd000..=0xdfff => Chip8Interpreter::inst_draw_sprite,
            0xe000..=0xefff => match inst & 0xff {
                0x9e => Chip8Interpreter::inst_skip_key_eq,
                0xa1 => Chip8Interpreter::inst_skip_key_ne,
                _ => panic!("Failed to decode instruction {:04x?}", inst),
            },
            0xf000..=0xffff => match inst & 0xff {
                0x07 => Chip8Interpreter::inst_get_timer,
                0x15 => Chip8Interpreter::inst_set_timer,
                0x1e => Chip8Interpreter::inst_add_x_to_i,
                0x29 => Chip8Interpreter::inst_load_char,
                0x33 => Chip8Interpreter::inst_x_to_bcd,
                0x55 => Chip8Interpreter::inst_save_v_at_i,
                0x65 => Chip8Interpreter::inst_load_v_at_i,
                _ => panic!("Failed to decode instruction {:04x?}", inst),
            },
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

    /// 3xnn
    fn inst_skip_vx_eq(&mut self) -> Result<usize, io::Error> {
        let lhs = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        let rhs = 0xff & self.instruction_data as u8;
        if lhs == rhs {
            self.program_counter += 2;
            Ok(14)
        } else {
            Ok(10)
        }
    }

    /// 4xnn
    fn inst_skip_vx_ne(&mut self) -> Result<usize, io::Error> {
        let lhs = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        let rhs = 0xff & self.instruction_data as u8;
        if lhs != rhs {
            self.program_counter += 2;
            Ok(14)
        } else {
            Ok(10)
        }
    }

    /// 5xy0
    fn inst_x_eq_y(&mut self) -> Result<usize, io::Error> {
        let lhs = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        let rhs = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        if lhs == rhs {
            self.program_counter += 2;
            Ok(18)
        } else {
            Ok(14)
        }
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

    /// 8xy0
    fn inst_load_x_with_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        self.memory
            .write(&[vy], self.memory.var_addr + self.vx, 1)?;
        Ok(12)
    }

    /// 8xy1
    fn inst_x_or_with_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        vx[0] |= vy;
        Ok(44)
    }

    /// 8xy2
    fn inst_x_and_with_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        vx[0] &= vy;
        Ok(44)
    }

    /// 8xy3
    fn inst_x_xor_with_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        vx[0] ^= vy;
        Ok(44)
    }

    /// 8xy4
    fn inst_x_add_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0] as u16;
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        let res: u16 = vx[0] as u16 + vy;
        vx[0] = 0xff & res as u8;
        self.memory.write(
            &[if res > 0xff { 0x01 } else { 0x00 }],
            self.memory.var_addr + 0xf,
            1,
        )?;
        Ok(44)
    }

    /// 8xy5
    fn inst_x_minus_y(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0] as u16;
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        let res: u16 = 0x100 + (vx[0] as u16) - vy;
        vx[0] = 0xff & res as u8;
        self.memory.write(
            &[if res < 0x100 { 0x00 } else { 0x01 }],
            self.memory.var_addr + 0xf,
            1,
        )?;
        Ok(44)
    }

    /// 8xy6
    fn inst_rshift_y_load_x(&mut self) -> Result<usize, io::Error> {
        // TODO variations
        // (see discussion here: https://laurencescotford.com/chip-8-on-the-cosmac-vip-arithmetic-and-logic-instructions/)
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        let res = vy >> 1;
        self.memory
            .write(&[res], self.memory.var_addr + self.vx, 1)?;
        self.memory
            .write(&[res], self.memory.var_addr + self.vy, 1)?;
        self.memory
            .write(&[vy & 0x1], self.memory.var_addr + 0xf, 1)?; // vf
        Ok(44)
    }

    /// 8xy7
    fn inst_y_minus_x(&mut self) -> Result<usize, io::Error> {
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0] as u16;
        let vx = self.memory.get_rw_slice(self.memory.var_addr + self.vx, 1);
        let res: u16 = 0x100 + vy - (vx[0] as u16);
        vx[0] = 0xff & res as u8;
        self.memory.write(
            &[if res < 0x100 { 0x00 } else { 0x01 }],
            self.memory.var_addr + 0xf,
            1,
        )?;
        Ok(44)
    }

    /// 8xye
    fn inst_lshift_y_load_x(&mut self) -> Result<usize, io::Error> {
        // TODO variations
        // (see discussion here: https://laurencescotford.com/chip-8-on-the-cosmac-vip-arithmetic-and-logic-instructions/)
        let vy = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        let res: u8 = (vy << 1) & 0xff;
        self.memory
            .write(&[res], self.memory.var_addr + self.vx, 1)?;
        self.memory
            .write(&[res], self.memory.var_addr + self.vy, 1)?;
        self.memory
            .write(&[(vy & 0x80) >> 7], self.memory.var_addr + 0xf, 1)?; // vf
        Ok(44)
    }

    /// 9xy0
    fn inst_x_ne_y(&mut self) -> Result<usize, io::Error> {
        let lhs = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        let rhs = self.memory.get_ro_slice(self.memory.var_addr + self.vy, 1)[0];
        if lhs != rhs {
            self.program_counter += 2;
            Ok(18)
        } else {
            Ok(14)
        }
    }

    /// annn
    fn inst_set_i(&mut self) -> Result<usize, io::Error> {
        self.i = self.instruction_data & 0xfff;
        Ok(12)
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

    /// cxnn
    fn inst_random(&mut self) -> Result<usize, io::Error> {
        // increment seed
        self.random = self.random.wrapping_add(1);

        // address for random number
        let rand_addr = 0x100 + (0xff & self.random);

        // fetch byte at rand address
        let rand_val = self.memory.get_ro_slice(rand_addr, 1)[0];

        // add to high-order byte of seed
        let rand_val = ((self.random >> 8) as u8).wrapping_add(rand_val);

        // div by 2 and add to itself
        let rand_val = (rand_val / 2).wrapping_add(rand_val);

        // save in top byte of seed
        self.random = (self.random & 0xff) + ((rand_val as u16) << 8);

        // mask with nn and store in vx
        self.memory.write(
            &[rand_val & (self.instruction_data & 0xff) as u8],
            self.memory.var_addr + self.vx,
            1,
        )?;

        Ok(36)
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

        // number of rows in the sprite
        let rows = 0xf & self.instruction_data as usize;

        // address to start drawing sprite in memory
        let draw_addr = vx_val / 8 // x byte offset
                      + vy_val * 8; // y byte offset

        // readable work area
        let work = self
            .memory
            .get_ro_slice(self.memory.work_addr, rows * 2)
            .to_vec();

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
            if this_addr >= vram.len() {
                // drawing off the bottom of the screen
                continue;
            }
            if idx % 2 == 1 && (this_addr & 0x3f) == 0 {
                // TODO and this
                // right-hand byte hangs off the edge of the screen
                continue;
            }
            if (vram[this_addr] & *byte) != 0x0 {
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

    /// ex9e
    fn inst_skip_key_eq(&mut self) -> Result<usize, io::Error> {
        let vx = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        match self.input.read_key() {
            Some(res) => match res {
                Err(e) => Err(e),
                Ok(key) =>
                    if vx == key {
                        self.program_counter += 2;
                        Ok(18)
                    } else {
                        Ok(14)
                    },
            },
            None => Ok(14),
        }
    }

    /// exa1
    fn inst_skip_key_ne(&mut self) -> Result<usize, io::Error> {
        let vx = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        match self.input.read_key() {
            Some(res) => match res {
                Err(e) => Err(e),
                Ok(key) =>
                    if vx == key {
                        Ok(14)
                    } else {
                        self.program_counter += 2;
                        Ok(18)
                    },
            },
            None => {
                self.program_counter += 2;
                Ok(18)
            },
        }
    }

    /// fx07
    fn inst_get_timer(&mut self) -> Result<usize, io::Error> {
        self.memory
            .write(&[self.general_timer], self.memory.var_addr + self.vx, 1)?;
        Ok(10)
    }

    /// fx15
    fn inst_set_timer(&mut self) -> Result<usize, io::Error> {
        self.general_timer = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        Ok(10)
    }

    /// fx1e
    fn inst_add_x_to_i(&mut self) -> Result<usize, io::Error> {
        let vx = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0] as u16;
        let old_i = self.i;
        self.i += vx;
        // 12+4 or 18+4; from https://laurencescotford.com/chip-8-on-the-cosmac-vip-indexing-the-memory/
        if (old_i & 0xff00) == (self.i & 0xff00) {
            Ok(16)
        } else {
            Ok(22)
        }
    }

    /// fx29
    fn inst_load_char(&mut self) -> Result<usize, io::Error> {
        let ch = 0xf & self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0] as u16;

        // since we have the _actual_ VIP interpreter in 0x000-0x1ff anyway for
        // authentic "randomness" ... we can use its lookup to get font addresses
        let lookup_addr = self.memory.get_ro_slice(0x8100 + ch, 1)[0] as u16;

        self.i = 0x8100 + lookup_addr;
        Ok(20)
    }

    /// fx33
    fn inst_x_to_bcd(&mut self) -> Result<usize, io::Error> {
        let input = self.memory.get_ro_slice(self.memory.var_addr + self.vx, 1)[0];
        let output = self.memory.get_rw_slice(self.i, 3);
        output[0] = input / 100;
        output[1] = (input % 100) / 10;
        output[2] = (input % 100) % 10;
        Ok(84 + 16 * ((output[0] + output[1] + output[2]) as usize))
    }

    /// fx55
    fn inst_save_v_at_i(&mut self) -> Result<usize, io::Error> {
        let v = self
            .memory
            .get_ro_slice(self.memory.var_addr, 1 + self.vx as usize)
            .to_vec();
        self.memory.write(v.as_slice(), self.i, v.len())?;

        // i points at address after i+vx
        self.i += self.vx + 1;
        // 14 + 14 * x + 4
        Ok(14 + 14 * (1 + self.vx as usize) + 4)
    }

    /// fx65
    fn inst_load_v_at_i(&mut self) -> Result<usize, io::Error> {
        let v = self
            .memory
            .get_ro_slice(self.i, 1 + self.vx as usize)
            .to_vec();
        self.memory
            .write(v.as_slice(), self.memory.var_addr, v.len())?;

        // i points at address after i+vx
        self.i += self.vx + 1;
        // 14 + 14 * x + 4
        Ok(14 + 14 * (1 + self.vx as usize) + 4)
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
        let mut input = input::DummyInput::new(&[0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]);
        let mut i = Chip8Interpreter::new(&mut display, &mut input)?;
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
    fn test_skip_vx_eq_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x34, 0x56];
            i.load_program(&mut m)?;
            i.memory.write(&[0x56], 0xef4, 1)?;

            // call 3456
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_vx_eq()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 14 cycles
            assert_eq!(t, 14);
            Ok(())
        })
    }

    #[test]
    fn test_skip_vx_eq_not() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x34, 0x56];
            i.load_program(&mut m)?;
            i.memory.write(&[0x57], 0xef4, 1)?;

            // call 3456
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_vx_eq()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
            Ok(())
        })
    }

    #[test]
    fn test_skip_vx_ne_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x44, 0x67];
            i.load_program(&mut m)?;
            i.memory.write(&[0x56], 0xef4, 1)?;

            // call 4467
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_vx_ne()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 14 cycles
            assert_eq!(t, 14);
            Ok(())
        })
    }

    #[test]
    fn test_skip_vx_ne_not() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x44, 0x67];
            i.load_program(&mut m)?;
            i.memory.write(&[0x67], 0xef4, 1)?;

            // call 4467
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_vx_ne()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
            Ok(())
        })
    }

    #[test]
    fn test_skip_x_eq_y_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x54, 0x50];
            i.load_program(&mut m)?;
            i.memory.write(&[0x56, 0x56], 0xef4, 2)?;

            // call 5450
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_eq_y()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 18 cycles
            assert_eq!(t, 18);
            Ok(())
        })
    }

    #[test]
    fn test_skip_x_eq_y_not() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x54, 0x50];
            i.load_program(&mut m)?;
            i.memory.write(&[0x57, 0x56], 0xef4, 2)?;

            // call 5450
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_eq_y()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 14 cycles
            assert_eq!(t, 14);
            Ok(())
        })
    }

    #[test]
    fn test_skip_x_ne_y_ok() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x94, 0x50];
            i.load_program(&mut m)?;
            i.memory.write(&[0x56, 0x57], 0xef4, 2)?;

            // call 9450
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_ne_y()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 18 cycles
            assert_eq!(t, 18);
            Ok(())
        })
    }

    #[test]
    fn test_skip_x_ne_y_not() -> Result<(), io::Error> {
        test_with(|i| {
            let mut m: &[u8] = &[0x94, 0x50];
            i.load_program(&mut m)?;
            i.memory.write(&[0x67, 0x67], 0xef4, 2)?;

            // call 9450
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_ne_y()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-skip-instructions/
            // takes 14 cycles
            assert_eq!(t, 14);
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
    fn test_load_x_with_y() -> Result<(), io::Error> {
        // 8xy0
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x20];
            i.load_program(&mut m)?;
            i.memory.write(&[0x11, 0x22], 0xef1, 2)?;

            // call 8120
            let _ = i.fetch_and_decode()?;
            let t = i.inst_load_x_with_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x22, 0x22]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 12 cycles
            assert_eq!(t, 12);
            Ok(())
        })
    }

    #[test]
    fn test_x_or_with_y() -> Result<(), io::Error> {
        // 8xy1
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x21];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8121
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_or_with_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x6f, 0x4b]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_and_with_y() -> Result<(), io::Error> {
        // 8xy2
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x22];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8122
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_and_with_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x09, 0x4b]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_xor_with_y() -> Result<(), io::Error> {
        // 8xy3
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x23];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8123
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_xor_with_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x66, 0x4b]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_add_y() -> Result<(), io::Error> {
        // 8xy4
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x24];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8124
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_add_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x78, 0x4b]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x00]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_add_y_carry() -> Result<(), io::Error> {
        // 8xy4
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x24];
            i.load_program(&mut m)?;
            i.memory.write(&[0xed, 0x4b], 0xef1, 2)?;

            // call 8124
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_add_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x38, 0x4b]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x01]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_minus_y() -> Result<(), io::Error> {
        // 8xy5
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x25];
            i.load_program(&mut m)?;
            i.memory.write(&[0x4b, 0x2d], 0xef1, 2)?;

            // call 8125
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_minus_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x1e, 0x2d]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x01]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_x_minus_y_borrow() -> Result<(), io::Error> {
        // 8xy5
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x25];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8125
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_minus_y()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0xe2, 0x4b]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x00]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_rshift_y_load_x_0lsb() -> Result<(), io::Error> {
        // 8xy6
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x26];
            i.load_program(&mut m)?;
            i.memory.write(&[0xff, 0x2c], 0xef1, 2)?;

            // call 8126
            let _ = i.fetch_and_decode()?;
            let t = i.inst_rshift_y_load_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x16, 0x16]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x00]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_rshift_y_load_x_1lsb() -> Result<(), io::Error> {
        // 8xy6
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x26];
            i.load_program(&mut m)?;
            i.memory.write(&[0xff, 0x2d], 0xef1, 2)?;

            // call 8126
            let _ = i.fetch_and_decode()?;
            let t = i.inst_rshift_y_load_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x16, 0x16]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x01]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_y_minus_x() -> Result<(), io::Error> {
        // 8xy7
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x27];
            i.load_program(&mut m)?;
            i.memory.write(&[0x2d, 0x4b], 0xef1, 2)?;

            // call 8127
            let _ = i.fetch_and_decode()?;
            let t = i.inst_y_minus_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x1e, 0x4b]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x01]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_y_minus_x_borrow() -> Result<(), io::Error> {
        // 8xy7
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x27];
            i.load_program(&mut m)?;
            i.memory.write(&[0x4b, 0x2d], 0xef1, 2)?;

            // call 8127
            let _ = i.fetch_and_decode()?;
            let t = i.inst_y_minus_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0xe2, 0x2d]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x00]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_lshift_y_load_x_0msb() -> Result<(), io::Error> {
        // 8xye
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x2e];
            i.load_program(&mut m)?;
            i.memory.write(&[0xff, 0x2d], 0xef1, 2)?;

            // call 812e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_lshift_y_load_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x5a, 0x5a]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x00]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
            Ok(())
        })
    }

    #[test]
    fn test_lshift_y_load_x_1msb() -> Result<(), io::Error> {
        // 8xye
        test_with(|i| {
            let mut m: &[u8] = &[0x81, 0x2e];
            i.load_program(&mut m)?;
            i.memory.write(&[0xff, 0xad], 0xef1, 2)?;

            // call 812e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_lshift_y_load_x()?;

            assert_eq!(i.memory.get_ro_slice(0xef1, 2), &[0x5a, 0x5a]);
            assert_eq!(i.memory.get_ro_slice(0xeff, 1), &[0x01]); // vf

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 44 cycles
            assert_eq!(t, 44);
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
    fn test_random_seed_inc_by_interrupt() -> Result<(), io::Error> {
        test_with(|i| {
            i.random = 0x1234;
            i.interrupt()?;
            assert_eq!(i.random, 0x1235);
            Ok(())
        })
    }

    #[test]
    fn test_random_logic() -> Result<(), io::Error> {
        // cxnn
        test_with(|i| {
            let mut m: &[u8] = &[0xc2, 0x03];
            i.load_program(&mut m)?;
            i.random = 0x0107;

            // call c203
            let _ = i.fetch_and_decode()?;
            let t = i.inst_random()?;

            // mem[1 + 0x0107 & 0xff] == 0x56
            // 56 + 01 == 57
            // 57/2+57 == 82

            assert_eq!(i.random, 0x8208);
            assert_eq!(i.memory.get_ro_slice(0xef2, 1), &[0x02]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-generating-random-numbers/
            // takes 36 cycles
            assert_eq!(t, 36);
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

            assert_eq!(t, 139);
            Ok(())
        })
    }

    #[test]
    fn test_key_skip_eq_none() -> Result<(), io::Error> {
        // ex9e
        test_with(|i| {
            let mut m: &[u8] = &[0xe2, 0x9e];
            i.load_program(&mut m)?;
            i.memory.write(&[0x0a], 0xef2, 1)?;
            while i.input.read_key().is_some() {};

            // call e29e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_key_eq()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 14 cycles
            assert_eq!(t, 14);
            Ok(())
        })
    }

    #[test]
    fn test_key_skip_eq_match() -> Result<(), io::Error> {
        // ex9e
        test_with(|i| {
            let mut m: &[u8] = &[0xe2, 0x9e];
            i.load_program(&mut m)?;
            i.memory.write(&[0x0f], 0xef2, 1)?;

            // call e29e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_key_eq()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 18 cycles
            assert_eq!(t, 18);
            Ok(())
        })
    }

    #[test]
    fn test_key_skip_ne_none() -> Result<(), io::Error> {
        // exa1
        test_with(|i| {
            let mut m: &[u8] = &[0xe2, 0xa1];
            i.load_program(&mut m)?;
            i.memory.write(&[0x0a], 0xef2, 1)?;
            while i.input.read_key().is_some() {};

            // call e2a1
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_key_ne()?;

            assert_eq!(i.program_counter, 0x204);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 14 cycles
            assert_eq!(t, 18);
            Ok(())
        })
    }

    #[test]
    fn test_key_skip_ne_match() -> Result<(), io::Error> {
        // exa1
        test_with(|i| {
            let mut m: &[u8] = &[0xe2, 0xa1];
            i.load_program(&mut m)?;
            i.memory.write(&[0x0f], 0xef2, 1)?;

            // call e2a1
            let _ = i.fetch_and_decode()?;
            let t = i.inst_skip_key_ne()?;

            assert_eq!(i.program_counter, 0x202);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 18 cycles
            assert_eq!(t, 14);
            Ok(())
        })

    }


    #[test]
    fn test_get_timer() -> Result<(), io::Error> {
        // fx07
        test_with(|i| {
            let mut m: &[u8] = &[0xf0, 0x07];
            i.load_program(&mut m)?;
            i.memory.write(&[0x80], 0xef0, 1)?;
            i.general_timer = 0x08;

            // call fx07
            let _ = i.fetch_and_decode()?;
            let t = i.inst_get_timer()?;

            assert_eq!(i.memory.get_ro_slice(0xef0, 1), &[0x08]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
            Ok(())
        })
    }

    #[test]
    fn test_set_timer() -> Result<(), io::Error> {
        // fx15
        test_with(|i| {
            let mut m: &[u8] = &[0xf0, 0x15];
            i.load_program(&mut m)?;
            i.memory.write(&[0x80], 0xef0, 1)?;
            i.general_timer = 0x08;

            // call fx15
            let _ = i.fetch_and_decode()?;
            let t = i.inst_set_timer()?;

            assert_eq!(i.general_timer, 0x80);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 10 cycles
            assert_eq!(t, 10);
            Ok(())
        })
    }

    #[test]
    fn test_interrupt_decrements_timer() -> Result<(), io::Error> {
        test_with(|i| {
            i.general_timer = 0x08;
            let t = i.interrupt()?;

            assert_eq!(i.general_timer, 0x07);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-branch-and-call-instructions/
            // takes 815 + 1024 cycles
            assert_eq!(t, 1839);
            Ok(())
        })
    }

    #[test]
    fn test_add_x_to_i() -> Result<(), io::Error> {
        // fx1e
        test_with(|i| {
            let mut m: &[u8] = &[0xf0, 0x1e];
            i.load_program(&mut m)?;
            i.memory.write(&[0x84], 0xef0, 1)?;
            i.i = 0x42;

            // call fx1e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_add_x_to_i()?;

            assert_eq!(i.i, 0xc6);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-indexing-the-memory/
            // takes 12+4 cycles
            assert_eq!(t, 16);
            Ok(())
        })
    }

    #[test]
    fn test_add_x_to_i_with_carry() -> Result<(), io::Error> {
        // fx1e
        test_with(|i| {
            let mut m: &[u8] = &[0xf0, 0x1e];
            i.load_program(&mut m)?;
            i.memory.write(&[0x84], 0xef0, 1)?;
            i.i = 0x82;

            // call fx1e
            let _ = i.fetch_and_decode()?;
            let t = i.inst_add_x_to_i()?;

            assert_eq!(i.i, 0x106);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-indexing-the-memory/
            // takes 18+4 cycles
            assert_eq!(t, 22);
            Ok(())
        })
    }

    #[test]
    fn test_load_char() -> Result<(), io::Error> {
        // fx29
        test_with(|i| {
            let mut m: &[u8] = &[0xf2, 0x29];
            i.load_program(&mut m)?;
            i.memory.write(&[0x0e], 0xef2, 1)?;

            // call f229
            let _ = i.fetch_and_decode()?;
            let t = i.inst_load_char()?;

            assert_eq!(i.i, 0x8110);

            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-the-character-set/
            // takes 18+4 cycles
            assert_eq!(t, 20);
            Ok(())
        })
    }

    #[test]
    fn test_x_to_bcd() -> Result<(), io::Error> {
        // fx33
        test_with(|i| {
            let mut m: &[u8] = &[0xf2, 0x33];
            i.load_program(&mut m)?;
            i.memory.write(&[0x7b], 0xef2, 1)?;
            i.i = 0x300;

            // call f233
            let _ = i.fetch_and_decode()?;
            let t = i.inst_x_to_bcd()?;

            assert_eq!(i.i, 0x300);
            assert_eq!(i.memory.get_ro_slice(i.i, 3), &[1, 2, 3]);
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-binary-coded-decimal/
            // takes 4 + 80 + (16 for each 1, 10, 100) cycles
            assert_eq!(t, 180);
            Ok(())
        })
    }

    #[test]
    fn test_save_v_at_i() -> Result<(), io::Error> {
        // fx55
        test_with(|i| {
            let mut m: &[u8] = &[0xff, 0x55];
            i.load_program(&mut m)?;
            i.memory.write(
                &[
                    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
                    0x1d, 0x1e, 0x1f,
                ],
                0xef0,
                16,
            )?;
            i.i = 0x300;

            // call fx55
            let _ = i.fetch_and_decode()?;
            let t = i.inst_save_v_at_i()?;

            assert_eq!(i.i, 0x310);
            assert_eq!(
                i.memory.get_ro_slice(0x300, 16),
                &[
                    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
                    0x1d, 0x1e, 0x1f
                ]
            );
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 238 + 4 cycles for 16 registers
            assert_eq!(t, 242);
            Ok(())
        })
    }

    #[test]
    fn test_load_v_at_i() -> Result<(), io::Error> {
        // fx65
        test_with(|i| {
            let mut m: &[u8] = &[0xff, 0x65];
            i.load_program(&mut m)?;
            i.memory.write(
                &[
                    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
                    0x1d, 0x1e, 0x1f,
                ],
                0x300,
                16,
            )?;
            i.i = 0x300;

            // call fx65
            let _ = i.fetch_and_decode()?;
            let t = i.inst_load_v_at_i()?;

            assert_eq!(i.i, 0x310);
            assert_eq!(
                i.memory.get_ro_slice(0xef0, 16),
                &[
                    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
                    0x1d, 0x1e, 0x1f
                ]
            );
            // from https://laurencescotford.com/chip-8-on-the-cosmac-vip-loading-and-saving-variables/
            // takes 238 + 4 cycles for 16 registers
            assert_eq!(t, 242);
            Ok(())
        })
    }
}
