use std::fs::File;
use std::io;

use chip8::display::MonoTermDisplay;
use chip8::input::StdinInput;
use chip8::interpreter::Chip8Interpreter;

fn main() -> Result<(), io::Error> {
    // initialise
    // TODO: decouple internal and external resolution; make interpreter responsible for former
    let mut display = MonoTermDisplay::new(64, 32)?;
    let mut input = StdinInput::new();
    let mut interpreter = Chip8Interpreter::new(&mut display, &mut input)?;

    // load a program
    //let mut f = File::open("roms/trip8_demo.ch8")?;
    //let mut f = File::open("roms/sqrt_test.ch8")?;
    //let mut f = File::open("roms/framed_2.ch8")?;
    let mut f = File::open("roms/submarine.ch8")?;

    interpreter.load_program(&mut f)?;
    interpreter.main_loop(1800)?;

    // test card for the display
    //display.test_card()?;

    // shove some junk on stdout to stop the cli messing up the last frame
    for _ in 0..12 {
        println!();
    }
    Ok(())
}
