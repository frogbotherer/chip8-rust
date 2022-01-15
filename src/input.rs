use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::terminal;
use std::collections::HashMap;
use std::io;
use std::time::Duration;

/// map of async bytes read from the keyboard to what the chip8 might expect
/// where '1' => 0x01 and 'a' => 0x0a
#[allow(dead_code)]
const CHIP8_LITERAL_KEYMAP: [(char, u8); 16] = [
    ('0', 0x00),
    ('1', 0x01),
    ('2', 0x02),
    ('3', 0x03),
    ('4', 0x04),
    ('5', 0x05),
    ('6', 0x06),
    ('7', 0x07),
    ('8', 0x08),
    ('9', 0x09),
    ('a', 0x0a),
    ('b', 0x0b),
    ('c', 0x0c),
    ('d', 0x0d),
    ('e', 0x0e),
    ('f', 0x0f),
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
    /// forget the latched key
    fn flush_keys(&mut self) -> Result<(), io::Error>;

    /// read the latched key
    fn read_key(&mut self) -> Result<Option<u8>, io::Error>;

    /// tell the input that a frame has passed
    fn tick(&mut self) -> Result<(), io::Error>;
}

/// simple implementation of Input, using STDIN
pub struct StdinInput {
    keymap: HashMap<char, u8>,
    latched_key: Option<u8>,
    timer: usize,
}

impl StdinInput {
    pub fn new() -> Self {
        terminal::enable_raw_mode().unwrap();
        StdinInput {
            keymap: HashMap::from(CHIP8_CONVENTIONAL_KEYMAP),
            latched_key: None,
            timer: STDIN_DEBOUNCE_FRAMES,
        }
    }

    fn read_stdin(&mut self) -> Result<(), io::Error> {
        while poll(Duration::from_millis(0))? {
            match read()? {
                Event::Key(evt) => match evt.code {
                    KeyCode::Char(key) => match self.keymap.get(&key) {
                        Some(mapped_key) => self.latched_key = Some(*mapped_key),
                        None => {
                            eprintln!("Warning: can't map {:02x?} to a COSMAC key", key);
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

/// how long to remember a keypress for
const STDIN_DEBOUNCE_FRAMES: usize = 30; // 1/2 second

impl Input for StdinInput {
    fn flush_keys(&mut self) -> Result<(), io::Error> {
        self.latched_key = None;
        Ok(())
    }

    fn read_key(&mut self) -> Result<Option<u8>, io::Error> {
        if self.latched_key == None {
            self.read_stdin()?;
        }
        Ok(self.latched_key)
    }

    fn tick(&mut self) -> Result<(), io::Error> {
        self.timer -= 1;
        if self.timer == 0 {
            self.flush_keys()?;
            self.read_stdin()?;
            self.timer = STDIN_DEBOUNCE_FRAMES;
        }
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
    fn flush_keys(&mut self) -> Result<(), io::Error> {
        self.bytes.clear();
        Ok(())
    }

    fn read_key(&mut self) -> Result<Option<u8>, io::Error> {
        Ok(self.bytes.pop())
    }

    fn tick(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}
