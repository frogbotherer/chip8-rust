use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::terminal;
use std::collections::HashMap;
use std::io;
use std::time::Duration;

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
const CHIP8_CONVENTIONAL_KEYMAP: [(char, u8); 16] = [
    ('x', 0x00), // x
    ('1', 0x01), // 1
    ('2', 0x02), // 2
    ('3', 0x03), // 3
    ('q', 0x04), // q
    ('w', 0x05), // w
    ('e', 0x06), // e
    ('a', 0x07), // a
    ('s', 0x08), // s
    ('d', 0x09), // d
    ('z', 0x0a), // z
    ('c', 0x0b), // c
    ('4', 0x0c), // 4
    ('r', 0x0d), // r
    ('f', 0x0e), // f
    ('v', 0x0f), // v
];

/// reads keypresses
pub trait Input {
    /// get a list of all the mapped keys that have been pressed recently,
    /// without flushing them from the buffer
    fn peek_keys(&mut self) -> Result<&[u8], io::Error>;

    /// flush all the keypresses from the buffer
    fn flush_keys(&mut self) -> Result<(), io::Error>;
}

/// simple implementation of Input, using STDIN
pub struct StdinInput {
    buffer: Vec<u8>,
    keymap: HashMap<char, u8>,
}

impl StdinInput {
    pub fn new() -> Self {
        terminal::enable_raw_mode().unwrap();
        StdinInput {
            buffer: Vec::new(),
            keymap: HashMap::from(CHIP8_CONVENTIONAL_KEYMAP),
        }
    }

    fn read_stdin(&mut self) -> Result<(), io::Error> {
        while poll(Duration::from_millis(0))? {
            match read()? {
                Event::Key(evt) => match evt.code {
                    KeyCode::Char(key) => match self.keymap.get(&key) {
                        Some(mapped_key) => self.buffer.push(*mapped_key),
                        None => {
                            eprintln!("Warning: can't map 0x{:02x?} to a COSMAC key", key);
                        }
                    },
                    KeyCode::Esc => panic!("TODO: proper emulator menus"),
                    _ => {
                        eprintln!("Warning: unknown key event received");
                    }
                },
                _ => {
                    eprintln!("Warning: unknown event received");
                }
            }
        }
        Ok(())
    }
}

impl Drop for StdinInput {
    fn drop(&mut self) {
        terminal::disable_raw_mode().unwrap();
    }
}

impl Input for StdinInput {
    fn peek_keys(&mut self) -> Result<&[u8], io::Error> {
        self.read_stdin()?;
        Ok(self.buffer.as_slice())
    }

    fn flush_keys(&mut self) -> Result<(), io::Error> {
        self.read_stdin()?;
        self.buffer.clear();
        Ok(())
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
    fn peek_keys(&mut self) -> Result<&[u8], io::Error> {
        Ok(self.bytes.as_slice())
    }

    fn flush_keys(&mut self) -> Result<(), io::Error> {
        self.bytes.clear();
        Ok(())
    }
}
