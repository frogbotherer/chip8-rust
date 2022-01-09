use beep::beep;
use std::error::Error;

pub trait Sound {
    fn beep(&mut self) -> Result<(), Box<dyn Error>>;
    fn stop(&mut self) -> Result<(), Box<dyn Error>>;
}

const SIMPLEBEEP_PITCH: u16 = 2093; // C

pub struct SimpleBeep {
    is_beeping: bool,
}

impl SimpleBeep {
    pub fn new() -> Self {
        SimpleBeep { is_beeping: false }
    }
}

impl Sound for SimpleBeep {
    fn beep(&mut self) -> Result<(), Box<dyn Error>> {
        beep(SIMPLEBEEP_PITCH)?;
        self.is_beeping = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        beep(0)?;
        self.is_beeping = false;
        Ok(())
    }
}

pub struct Mute {}
impl Mute {
    pub fn new() -> Self {
        Mute {}
    }
}
impl Sound for Mute {
    fn beep(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
