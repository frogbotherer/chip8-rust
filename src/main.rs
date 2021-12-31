use std::fs::File;
use std::{time,thread};
use std::io;

use chip8::display::MonoTermDisplay;
use chip8::interpreter::Chip8Interpreter;

fn main() -> Result<(), io::Error> {
    // initialise
    // TODO: decouple internal and external resolution; make interpreter responsible for former
    let mut display = MonoTermDisplay::new(64, 32)?;
    let mut interpreter = Chip8Interpreter::new(&mut display)?;

    // load a program
    let mut f = File::open("roms/ibm_logo.ch8")?;
    interpreter.load_program(&mut f)?;

    for i in 0..1000 {
        let t = interpreter.cycle()?;
        //eprintln!("{} {:04x?} ", i, interpreter.instruction_data);
        // 4.54us per machine cycle
        thread::sleep(time::Duration::from_nanos(4540 * t as u64));

        if i % 20 == 0 {
            interpreter.interrupt()?;
        }
    }
    //display.draw(&CHIP8_TEST_CARD)?;
    for _ in 0..12 {
        // shove some junk on stdout to stop the cli messing up the last frame
        eprintln!();
    }
    Ok(())
}
