use std::io;
use std::fs::File;

use chip8::display::{Display, MonoTermDisplay, CHIP8_TEST_CARD};
use chip8::memory::MemoryMap;

fn main() -> Result<(), io::Error> {
    // initialise
    let mut display = MonoTermDisplay::new(64, 32)?;
    let mut memory = MemoryMap::new(4096);

    // load a program
    let mut f = File::open("roms/ibm_logo.ch8")?;
    memory.write_any(&mut f, 0x200)?;

    //loop {
    display.draw(&CHIP8_TEST_CARD)?;
    //thread::sleep(time::Duration::from_millis(10000));
    //}
    Ok(())
}
