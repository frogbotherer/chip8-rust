use std::error::Error;
use std::fs::File;

use chip8::display::MonoTermDisplay;
use chip8::input::StdinInput;
use chip8::interpreter::Chip8Interpreter;
use chip8::sound::Mute;

fn main() -> Result<(), Box<dyn Error>> {
    // initialise
    // TODO: decouple internal and external resolution; make interpreter responsible for former
    let mut display = MonoTermDisplay::new(64, 32)?;
    let mut input = StdinInput::new();
    let mut sound = Mute::new();
    let mut interpreter = Chip8Interpreter::new(&mut display, &mut input, &mut sound)?;

    // load a program
    let mut f = File::open("roms/trip8_demo.ch8")?;
    //let mut f = File::open("roms/sqrt_test.ch8")?;
    //let mut f = File::open("roms/submarine.ch8")?; // problem with sprite rendering still?
    //let mut f = File::open("roms/hi_lo.ch8")?; // f10a

    interpreter.load_program(&mut f)?;
    interpreter.main_loop(18_000)?;

    // test card for the display
    //display.test_card()?;

    // shove some junk on stdout to stop the cli messing up the last frame
    for _ in 0..12 {
        println!();
    }
    Ok(())
}
