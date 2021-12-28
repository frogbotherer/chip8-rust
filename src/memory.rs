use std::io;
/// Represents memory map, ROM, RAM etc.

pub struct MemoryMap {
    // TODO should this be a boxed slice instead?
    bytes: Vec<u8>,
}

impl MemoryMap {
    pub fn new(size: usize) -> MemoryMap {
        MemoryMap {
            bytes: vec![0; size],
        }
    }

    /// write unknown len of data into memory at a particular address
    pub fn write_any(
        &mut self,
        reader: &mut impl io::Read,
        address: usize,
    ) -> Result<(), io::Error> {
        // there's probably a considerably slicker way of splicing a bunch of
        // u8 from disk into a chunk of RAM
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        self.write(buf, address)
    }

    // write vector into "RAM"
    pub fn write(&mut self, data: Vec<u8>, address: usize) -> Result<(), io::Error> {
        let splice_end = address + data.len();
        assert!(splice_end <= self.bytes.len(), "Memory overrun");
        let _: Vec<_> = self.bytes.splice(address..splice_end, data).collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_zeroed() {
        let m = MemoryMap::new(16);
        for mi in m.bytes {
            assert_eq!(mi, 0);
        }
    }

    #[test]
    fn test_read_data_ok() -> Result<(), io::Error> {
        let mut dst = MemoryMap::new(16);
        let mut src: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
        dst.write_any(&mut src, 8)?;
        assert_eq!(dst.bytes, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7]);
        Ok(())
    }

    #[test]
    #[should_panic]
    fn test_read_too_much_panic() {
        let mut dst = MemoryMap::new(16);
        let mut src: &[u8] = &[0; 8];
        let _ = dst.write_any(&mut src, 9);
    }
}
