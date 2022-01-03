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
const CHIP8_STACK_OFFSET: u16 = 0x0132; // not! 0x0160; stack grows downward into real memory
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
        //mm.write(
        //    &CHIP8_CONTEMPORARY_FONT,
        //    CHIP8_CONTEMPORARY_FONT_ADDR,
        //    CHIP8_CONTEMPORARY_FONT.len(),
        //)?;
        mm.write(&CHIP8_INTERPRETER_SOURCE, 0x0, 0x200)?;
        Ok(mm)
    }

    /// load a CHIP-8 program at 0x200
    pub fn load_program(&mut self, reader: &mut impl io::Read) -> Result<(), io::Error> {
        self.write_any(reader, self.program_addr)
    }
}

#[allow(dead_code)]
const CHIP8_CONTEMPORARY_FONT_ADDR: u16 = 0x050;
#[allow(dead_code)]
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

// from the cosmac vip manual
// https://www.old-computers.com/download/rca/RCA_COSMAC_VIP-Instruction_Manual_for_VP-111.pdf
const CHIP8_INTERPRETER_SOURCE: [u8; 0x200] = [
    0x91, 0xbb, 0xff, 0x01, 0xb2, 0xb6, 0xf6, 0xcf, // 0000
    0xa2, 0xf8, 0x81, 0xb1, 0xf8, 0x46, 0xa1, 0x90, 0xb4, 0xf8, 0x1b, 0xa4, 0xf8, 0x01, 0xb5, 0xf8,
    0xfc, 0xa5, 0xd4, 0x96, 0xb7, 0xe2, 0x94, 0xbc, 0x45, 0xaf, 0xf6, 0xf6, 0xf6, 0xf6, 0x32, 0x44,
    0xf9, 0x50, 0xac, 0x8f, 0xfa, 0x0f, 0xf9, 0xf0, 0xa6, 0x05, 0xf6, 0xf6, 0xf6, 0xf6, 0xf9,
    0xf0, // 0030
    0xa7, 0x4c, 0xb3, 0xbc, 0xfc, 0x0f, 0xac, 0x0c, 0xa3, 0xd3, 0x30, 0x1b, 0x8f, 0xfa, 0x0f, 0xb3,
    0x45, 0x30, 0x40, 0x22, 0x69, 0x12, 0xd4, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x7c, 0x75, 0x83, 0x8b, 0x95, 0xb4,
    0xb7, // 0060
    0xbc, 0x91, 0xeb, 0xa4, 0xd9, 0x70, 0x99, 0x05, 0x06, 0xfa, 0x07, 0xbe, 0x06, 0xfa, 0x3f, 0xf6,
    0xf6, 0xf6, 0x22, 0x52, 0x07, 0xfa, 0x1f, 0xfe, 0xfe, 0xfe, 0xf1, 0xac, 0x9b, 0xbc, 0x45, 0xfa,
    0x0f, 0xad, 0xa7, 0xf8, 0xd0, 0xa6, 0x93, 0xaf, 0x87, 0x32, 0xf3, 0x27, 0x4a, 0xbd, 0x9e,
    0xae, // 0090
    0x8e, 0x32, 0xa4, 0x9d, 0xf6, 0xbd, 0x8f, 0x76, 0xaf, 0x2e, 0x30, 0x98, 0x9d, 0x56, 0x16, 0x8f,
    0x56, 0x16, 0x30, 0x8e, 0x00, 0xec, 0xf8, 0xd0, 0xa6, 0x93, 0xa7, 0x8d, 0x32, 0xd9, 0x06, 0xf2,
    0x2d, 0x32, 0xbe, 0xf8, 0x01, 0xa7, 0x46, 0xf3, 0x5c, 0x02, 0xfb, 0x07, 0x32, 0xd2, 0x1c,
    0x06, // 00c0
    0xf2, 0x32, 0xce, 0xf8, 0x01, 0xa7, 0x06, 0xf3, 0x5c, 0x2c, 0x16, 0x8c, 0xfc, 0x08, 0xac, 0x3b,
    0xb3, 0xf8, 0xff, 0xa6, 0x87, 0x56, 0x12, 0xd4, 0x9b, 0xbf, 0xf8, 0xff, 0xaf, 0x93, 0x5f, 0x8f,
    0x32, 0xdf, 0x2f, 0x30, 0xe5, 0x00, 0x42, 0xb5, 0x42, 0xa5, 0xd4, 0x8d, 0xa7, 0x87, 0x32,
    0xac, // 00f0
    0x2a, 0x27, 0x30, 0xf5, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x45, 0xa3, 0x98,
    0x56, 0xd4, 0xf8, 0x81, 0xbc, 0xf8, 0x95, 0xac, 0x22, 0xdc, 0x12, 0x56, 0xd4, 0x06, 0xb8, 0xd4,
    0x06, 0xa8, 0xd4, 0x64, 0x0a, 0x01, 0xe6, 0x8a, 0xf4, 0xaa, 0x3b, 0x28, 0x9a, 0xfc, 0x01,
    0xba, // 0120
    0xd4, 0xf8, 0x91, 0xba, 0x06, 0xfa, 0x0f, 0xaa, 0x0a, 0xaa, 0xd5, 0xe6, 0x06, 0xbf, 0x93, 0xbe,
    0xf8, 0x1b, 0xae, 0x2a, 0x1a, 0xf8, 0x00, 0x5a, 0x0e, 0xf5, 0x3b, 0x4b, 0x56, 0x0a, 0xfc, 0x01,
    0x5a, 0x30, 0x40, 0x4e, 0xf6, 0x3b, 0x3c, 0x9f, 0x56, 0x2a, 0x2a, 0xd4, 0x00, 0x22, 0x86,
    0x52, // 0150
    0xf8, 0xf0, 0xa7, 0x07, 0x5a, 0x87, 0xf3, 0x17, 0x1a, 0x3a, 0x5b, 0x12, 0xd4, 0x22, 0x86, 0x52,
    0xf8, 0xf0, 0xa7, 0x0a, 0x57, 0x87, 0xf3, 0x17, 0x1a, 0x3a, 0x6b, 0x12, 0xd4, 0x15, 0x85, 0x22,
    0x73, 0x95, 0x52, 0x25, 0x45, 0xa5, 0x86, 0xfa, 0x0f, 0xb5, 0xd4, 0x45, 0xe6, 0xf3, 0x3a,
    0x82, // 0180
    0x15, 0x15, 0xd4, 0x45, 0xe6, 0xf3, 0x3a, 0x88, 0xd4, 0x45, 0x07, 0x30, 0x8c, 0x45, 0x07, 0x30,
    0x84, 0xe6, 0x62, 0x26, 0x45, 0xa3, 0x36, 0x88, 0xd4, 0x3e, 0x88, 0xd4, 0xf8, 0xf0, 0xa7, 0xe7,
    0x45, 0xf4, 0xa5, 0x86, 0xfa, 0x0f, 0x3b, 0xb2, 0xfc, 0x01, 0xb5, 0xd4, 0x45, 0x56, 0xd4,
    0x45, // 01b0
    0xe6, 0xf4, 0x56, 0xd4, 0x45, 0xfa, 0x0f, 0x3a, 0xc4, 0x07, 0x56, 0xd4, 0xaf, 0x22, 0xf8, 0xd3,
    0x73, 0x8f, 0xf9, 0xf0, 0x52, 0xe6, 0x07, 0xd2, 0x56, 0xf8, 0xff, 0xa6, 0xf8, 0x00, 0x7e, 0x56,
    0xd4, 0x19, 0x89, 0xae, 0x93, 0xbe, 0x99, 0xee, 0xf4, 0x56, 0x76, 0xe6, 0xf4, 0xb9, 0x56,
    0x45, // 01e0
    0xf2, 0x56, 0xd4, 0x45, 0xaa, 0x86, 0xfa, 0x0f, 0xba, 0xd4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0xe0, 0x00, 0x4b,
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
        assert_eq!(m.stack_addr, 0x0ece);
        assert_eq!(m.work_addr, 0x0ed0);
        assert_eq!(m.var_addr, 0x0ef0);
        assert_eq!(m.display_addr, 0x0f00);
    }
}
