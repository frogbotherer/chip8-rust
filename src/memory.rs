use std::io;
use std::io::Read;

// NB. addresses are u16 as per the chip-8; lengths are usize to stop endless casting

/// Represents memory map, ROM, RAM etc.
pub trait MemoryMap {
    /// write unknown len of data into memory at a particular address
    fn write_any(&mut self, reader: &mut impl io::Read, addr: u16) -> Result<(), io::Error> {
        // there's probably a considerably slicker way of figuring out the
        // length of what we're reading
        let mut buf = Vec::new();
        let len = reader.read_to_end(&mut buf)?;
        self.write(buf.as_slice(), addr, len)
    }

    /// write a chunk of bytes into "RAM"
    fn write(&mut self, data: &[u8], addr: u16, len: usize) -> Result<(), io::Error> {
        let bytes = self.get_rw_slice(addr, len);
        let mut d: &[u8] = data;
        d.read(bytes)?;
        Ok(())
    }

    /// get a two-byte word (stack)
    fn get_word(&mut self, addr: u16) -> u16 {
        let word = self.get_ro_slice(addr, 2);
        ((word[0] as u16) << 8) + (word[1] as u16)
    }

    /// get a r/w slice of the underlying memory (heap)
    fn get_rw_slice(&mut self, addr: u16, len: usize) -> &mut [u8];

    /// get a r/o slice of the underlying memory (heap)
    fn get_ro_slice(&self, addr: u16, len: usize) -> &[u8];
}

/// Defines the CHIP-8 standard memory map
/// 2K configuration:
///   0x0000-0x01ff  interpreter
///   0x0200-0x069f  program
///   0x06a0-0x06cf  stack
///   0x06d0-0x06ef  work area
///   0x06f0-0x06ff  chip-8 variables
///   0x0700-0x07ff  display
///
/// 4K configuration:
///   0x0000-0x01ff  interpreter
///   0x0200-0x0e9f  program
///   0x0ea0-0x0ecf  stack
///   0x0ed0-0x0eef  work area
///   0x0ef0-0x0eff  chip-8 variables
///   0x0f00-0x0fff  display
///
/// chip-8 programs *should* not access these directly
pub struct Chip8MemoryMap {
    bytes: Box<[u8]>,
    pub program_addr: u16,
    pub stack_addr: u16,
    pub work_addr: u16,
    pub var_addr: u16,
    pub display_addr: u16,
}

impl MemoryMap for Chip8MemoryMap {
    fn get_rw_slice(&mut self, addr: u16, len: usize) -> &mut [u8] {
        let a = addr as usize;
        &mut self.bytes[a..(a + len)]
    }
    fn get_ro_slice(&self, addr: u16, len: usize) -> &[u8] {
        let a = addr as usize;
        &self.bytes[a..(a + len)]
    }
}

/// how much RAM we have
const CHIP8_RAM_SIZE_BYTES: u16 = 4096;

/// offsets from the top of RAM
const CHIP8_STACK_OFFSET: u16 = 0x0131; // not! 0x0160; stack grows downward into real memory
const CHIP8_WORK_OFFSET: u16 = 0x0130;
const CHIP8_VAR_OFFSET: u16 = 0x0110;
const CHIP8_DISPLAY_OFFSET: u16 = 0x100;

/// where the program is loaded
const CHIP8_PROGRAM_ADDR: u16 = 0x0200;

impl Chip8MemoryMap {
    /// initialises CHIP-8 with contemporary memory contents
    pub fn new() -> Result<Self, io::Error> {
        let mut mm = Chip8MemoryMap {
            bytes: Box::new([0u8; CHIP8_RAM_SIZE_BYTES as usize]),
            program_addr: CHIP8_PROGRAM_ADDR,
            stack_addr: CHIP8_RAM_SIZE_BYTES - CHIP8_STACK_OFFSET,
            work_addr: CHIP8_RAM_SIZE_BYTES - CHIP8_WORK_OFFSET,
            var_addr: CHIP8_RAM_SIZE_BYTES - CHIP8_VAR_OFFSET,
            display_addr: CHIP8_RAM_SIZE_BYTES - CHIP8_DISPLAY_OFFSET,
        };
        mm.write(
            &CHIP8_CONTEMPORARY_FONT,
            CHIP8_CONTEMPORARY_FONT_ADDR,
            CHIP8_CONTEMPORARY_FONT.len(),
        )?;
        Ok(mm)
    }

    /// load a CHIP-8 program at 0x200
    pub fn load_program(&mut self, reader: &mut impl io::Read) -> Result<(), io::Error> {
        self.write_any(reader, self.program_addr)
    }
}

const CHIP8_CONTEMPORARY_FONT_ADDR: u16 = 0x050;
const CHIP8_CONTEMPORARY_FONT: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80, // F
];

// from https://laurencescotford.com/chip-8-on-the-cosmac-vip-the-character-set/
#[allow(dead_code)]
const CHIP8_ORIGINAL_FONT: [u8; 51] = [
    0xF0, 0x80, 0xF0, 0x80, // E and F
    0xF0, 0x80, 0x80, 0x80, // F and C
    0xF0, 0x50, 0x70, 0x50, // B
    0xF0, 0x50, 0x50, 0x50, // D
    0xF0, 0x80, 0xF0, 0x10, // 5
    0xF0, 0x80, 0xF0, 0x90, // 6 and 8
    0xF0, 0x90, 0xF0, 0x10, // 9 and 3
    0xF0, 0x10, 0xF0, 0x90, // 3 and A
    0xF0, 0x90, 0x90, 0x90, // A and 0
    0xF0, 0x10, 0x10, 0x10, 0x10, // 7
    0x60, 0x20, 0x20, 0x20, 0x70, // 1
    0xA0, 0xA0, 0xF0, 0x20, 0x20, // 4
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_zeroed() -> Result<(), io::Error> {
        let m = Chip8MemoryMap::new()?;
        // NB. memory is zeroed from 0x200 because before that we bake in the
        //     font and other interpreter details
        assert_eq!(m.bytes[0x200..], [0; 0xe00]);
        Ok(())
    }

    #[test]
    fn test_write_any_data_ok() -> Result<(), io::Error> {
        let mut dst = Chip8MemoryMap::new()?;
        let mut src: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
        dst.write_any(&mut src, 8)?;
        assert_eq!(
            dst.bytes[..16],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7]
        );
        Ok(())
    }

    #[test]
    fn test_write_slice_ok() {
        let mut dst = Chip8MemoryMap::new().unwrap();
        let src: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
        dst.write(&src, 8, 8).unwrap();
        assert_eq!(
            dst.bytes[..16],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn test_read_ro() {
        let m = Chip8MemoryMap::new().unwrap();
        let s = m.get_ro_slice(0, 8);
        assert_eq!(s, &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_read_word() {
        let mut m = Chip8MemoryMap::new().unwrap();
        let mut src: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
        m.write(&mut src, 0, 8).unwrap();
        assert_eq!(m.get_word(0x4), 0x0405);
    }

    #[test]
    #[should_panic]
    fn test_read_too_much_panic() {
        let mut dst = Chip8MemoryMap::new().unwrap();
        let mut src: &[u8] = &[0; 8];
        let _ = dst.write_any(&mut src, 4089);
    }

    #[test]
    fn test_program_load_ok() -> Result<(), io::Error> {
        let mut dst = Chip8MemoryMap::new()?;
        let mut prog: &[u8] = &[0x00, 0xe0]; // clear screen
        dst.load_program(&mut prog)?;
        assert_eq!(dst.get_ro_slice(0x200, 2), &[0x00, 0xe0]);
        Ok(())
    }

    #[test]
    fn test_mem_layout() {
        let m = Chip8MemoryMap::new().unwrap();
        assert_eq!(m.stack_addr, 0x0ecf);
        assert_eq!(m.work_addr, 0x0ed0);
        assert_eq!(m.var_addr, 0x0ef0);
        assert_eq!(m.display_addr, 0x0f00);
    }
}
