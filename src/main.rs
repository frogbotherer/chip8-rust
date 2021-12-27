use std::{io, thread, time};

use chip8::display::{Display, MonoTermDisplay, CHIP8_TEST_CARD};

fn main() -> Result<(), io::Error> {
    let mut display = MonoTermDisplay::new(64, 32)?;
    //loop {
    display.draw(&CHIP8_TEST_CARD)?;
    //thread::sleep(time::Duration::from_millis(10000));
    //}
    Ok(())
}
