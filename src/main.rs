use std::fs::File;
use std::io;

use chip8::display::MonoTermDisplay;
//use chip8::display::DummyDisplay;
use chip8::interpreter::Chip8Interpreter;

fn main() -> Result<(), io::Error> {
    // initialise
    // TODO: decouple internal and external resolution; make interpreter responsible for former
    let mut display = MonoTermDisplay::new(64, 32)?;
    //let mut display = DummyDisplay::new()?;
    let mut interpreter = Chip8Interpreter::new(&mut display)?;

    // load a program
    let mut f = File::open("roms/ibm_logo.ch8")?;
    interpreter.load_program(&mut f)?;
    interpreter.main_loop(300)?;
    //display.draw(&CHIP8_TEST_CARD)?;
    for _ in 0..12 {
        // shove some junk on stdout to stop the cli messing up the last frame
        println!();
    }
    Ok(())
}
