use std::collections::HashMap;
use std::io;
use std::io::Read;
use termion::{async_stdin, AsyncReader};

/// map of async bytes read from the keyboard to what the chip8 might expect
/// where '1' => 0x01 and 'a' => 0x0a
#[allow(dead_code)]
const CHIP8_LITERAL_KEYMAP: [(u8, u8); 16] = [
    (0x30, 0x00),
    (0x31, 0x01),
    (0x32, 0x02),
    (0x33, 0x03),
    (0x34, 0x04),
    (0x35, 0x05),
    (0x36, 0x06),
    (0x37, 0x07),
    (0x38, 0x08),
    (0x39, 0x09),
    (0x61, 0x0a),
    (0x62, 0x0b),
    (0x63, 0x0c),
    (0x64, 0x0d),
    (0x65, 0x0e),
    (0x66, 0x0f),
];

/// ditto using left-hand side of qwerty keyboard
const CHIP8_CONVENTIONAL_KEYMAP: [(u8, u8); 16] = [
    (0x78, 0x00), // x
    (0x31, 0x01), // 1
    (0x32, 0x02), // 2
    (0x33, 0x03), // 3
    (0x71, 0x04), // q
    (0x77, 0x05), // w
    (0x65, 0x06), // e
    (0x61, 0x07), // a
    (0x73, 0x08), // s
    (0x64, 0x09), // d
    (0x7a, 0x0a), // z
    (0x63, 0x0b), // c
    (0x34, 0x0c), // 4
    (0x72, 0x0d), // r
    (0x66, 0x0e), // f
    (0x76, 0x0f), // v
];

/// reads keypresses
pub trait Input {
    /// non-blocking keypress 0x00-0x0f (or None)
    fn read_key(&mut self) -> Option<Result<u8, io::Error>>;
}

/// simple implementation of Input, using STDIN
pub struct StdinInput {
    stdin: io::Bytes<AsyncReader>,
    keymap: HashMap<u8, u8>,
}

impl StdinInput {
    pub fn new() -> Self {
        StdinInput {
            stdin: async_stdin().bytes(),
            keymap: HashMap::from(CHIP8_CONVENTIONAL_KEYMAP),
        }
    }
}

impl Input for StdinInput {
    fn read_key(&mut self) -> Option<Result<u8, io::Error>> {
        match self.stdin.next() {
            Some(res) => match res {
                Ok(key) => match self.keymap.get(&key) {
                    Some(mapped_key) => Some(Ok(*mapped_key)),
                    None => None,
                },
                Err(e) => Some(Err(e)),
            },
            None => None,
        }
    }
}

/// dummy Input implementation for testing
pub struct DummyInput {
    bytes: Vec<u8>,
}

impl DummyInput {
    pub fn new(keys: &[u8]) -> Self {
        DummyInput {
            bytes: Vec::from(keys),
        }
    }
}

impl Input for DummyInput {
    fn read_key(&mut self) -> Option<Result<u8, io::Error>> {
        let ret = self.bytes.pop();
        match ret {
            None => None,
            Some(c) => Some(Ok(c)),
        }
    }
}
