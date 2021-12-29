use std::fs::File;
use std::io;

use chip8::display::{Display, MonoTermDisplay, CHIP8_TEST_CARD};
use chip8::interpreter::Chip8Interpreter;

fn main() -> Result<(), io::Error> {
    // initialise
    // TODO: decouple internal and external resolution; make interpreter responsible for former
    let mut display = MonoTermDisplay::new(64, 32)?;
    let mut interpreter = Chip8Interpreter::new(&mut display)?;

    // load a program
    let mut f = File::open("roms/ibm_logo.ch8")?;
    interpreter.load_program(&mut f)?;

    //loop {
    //thread::sleep(time::Duration::from_millis(3000));
    interpreter.interrupt()?;
    //display.draw(&CHIP8_TEST_CARD)?;
    //}
    Ok(())
}
