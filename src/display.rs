use std::io;
use termion::raw::{IntoRawMode, RawTerminal};
use tui::backend::TermionBackend;
use tui::layout::Rect;
use tui::style::{Color, Style};
use tui::symbols::Marker;
use tui::widgets::canvas::{Canvas, Points};
use tui::widgets::{Block, Borders};
use tui::Terminal;

/// Display is used by the interpreter to draw things on the screen. It should
/// abstract the implementation details, so a variety of kinds of screen would
/// work.
pub trait Display {
    /// draw data based on internal resolution of display
    fn draw(&mut self, data: &[u8]) -> Result<(), io::Error>;

    /// how big the display data should be
    fn get_display_size_bytes(&mut self) -> usize;
}

// store useful metadata about the terminal
struct Resolution(usize, usize, usize);

impl Resolution {
    fn pixel_count(&self) -> usize {
        self.0 * self.1
    }
    fn byte_count(&self) -> usize {
        self.0 * self.1 * self.2 / 8
    }

    fn x_bounds(&self) -> [f64; 2] {
        [0.0, (self.0 - 1) as f64]
    }

    fn y_bounds(&self) -> [f64; 2] {
        [-1.0 * (self.1 - 1) as f64, 0.0]
    }

    #[allow(dead_code)]
    fn points_from_data<'a>(
        &self,
        data: &'a [u8],
    ) -> impl std::iter::Iterator<Item = (f64, f64, Color)> + 'a {
        let mut count = self.pixel_count();
        let w = self.0;
        std::iter::from_fn(move || {
            match count {
                0 => None,
                _ => {
                    count -= 1;
                    let bit = 1 & (data[count / 8] >> (7 - count % 8));
                    Some((
                        (count % w) as f64,        // x
                        -1.0 * (count / w) as f64, // y
                        if bit == 1 { Color::White } else { Color::Black },
                    ))
                }
            }
        })
    }

    fn bitplane_from_data<'a>(
        &self,
        data: &'a [u8],
        bitplane: u8,
    ) -> impl std::iter::Iterator<Item = (f64, f64)> + 'a {
        let mut count = self.pixel_count();
        let w = self.0;
        std::iter::from_fn(move || {
            while count > 0 {
                count -= 1;
                let bit = 1 & (data[count / 8] >> (7 - count % 8));
                if bit == bitplane {
                    return Some((
                        (count % w) as f64,        // x
                        -1.0 * (count / w) as f64, // y
                    ));
                }
            }
            None
        })
    }
}

/// monochrome display in a terminal, rendered using TUI and Termion
pub struct MonoTermDisplay {
    terminal: Terminal<TermionBackend<RawTerminal<io::Stdout>>>,
    resolution: Resolution,
}

impl MonoTermDisplay {
    pub fn new(x: usize, y: usize) -> Result<MonoTermDisplay, io::Error> {
        let stdout = io::stdout().into_raw_mode()?;
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(MonoTermDisplay {
            terminal,
            resolution: Resolution(x, y, 1),
        })
    }

    pub fn test_card(&mut self) -> Result<(), io::Error> {
        self.draw(&CHIP8_TEST_CARD)
    }
}

impl Display for MonoTermDisplay {
    fn draw(&mut self, data: &[u8]) -> Result<(), io::Error> {
        // make sure we're given exactly the right amount of data to draw
        assert_eq!(
            data.len(),
            self.resolution.byte_count(),
            "MonoTermDisplay must have correct-sized data to draw"
        );
        // i don't know how to draw things that aren't mono
        assert_eq!(
            self.resolution.2, 1,
            "MonoTermDisplay can only render one bitplane"
        );

        // for now this assumes a 1:1 ratio between terminal, chip8 and the
        // internal TUI canvas
        self.terminal.draw(|f| {
            let size = Rect::new(
                0,
                0,
                2 + self.resolution.0 as u16,
                2 + self.resolution.1 as u16,
            );

            let canvas = Canvas::default()
                .block(
                    Block::default()
                        .title("CHIP-8")
                        .borders(Borders::ALL)
                        .style(Style::default().bg(Color::Black)),
                )
                .x_bounds(self.resolution.x_bounds())
                .y_bounds(self.resolution.y_bounds())
                .marker(Marker::Block) //Braille
                .paint(|ctx| {
                    // expand each bitplane into x, y float coords, suitable for
                    // rendering with TUI. this just prints blocky points for now
                    ctx.draw(&Points {
                        coords: &self
                            .resolution
                            .bitplane_from_data(&data, 0)
                            .collect::<Vec<_>>(),
                        color: Color::Black,
                    });
                    ctx.draw(&Points {
                        coords: &self
                            .resolution
                            .bitplane_from_data(&data, 1)
                            .collect::<Vec<_>>(),
                        color: Color::White,
                    });
                });
            f.render_widget(canvas, size);
        })?;
        Ok(())
    }

    /// how big the display data should be
    fn get_display_size_bytes(&mut self) -> usize {
        self.resolution.byte_count()
    }
}

/// useful for testing non-display routines
pub struct DummyDisplay;

impl DummyDisplay {
    #[allow(dead_code)]
    pub fn new() -> Result<DummyDisplay, io::Error> {
        Ok(DummyDisplay {})
    }
}

impl Display for DummyDisplay {
    #[allow(unused)]
    fn draw(&mut self, data: &[u8]) -> Result<(), io::Error> {
        Ok(())
    }
    fn get_display_size_bytes(&mut self) -> usize {
        0x100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Resolution tests
    #[test]
    fn test_pixel_count() {
        let r = Resolution(64, 32, 1);
        assert_eq!(r.pixel_count(), 2048)
    }

    #[test]
    fn test_byte_count() {
        let r = Resolution(64, 32, 1);
        assert_eq!(r.byte_count(), 256)
    }

    #[test]
    fn test_x_bounds() {
        let r = Resolution(64, 32, 1);
        assert_eq!(r.x_bounds(), [0.0, 63.0]);
    }

    #[test]
    fn test_y_bounds() {
        let r = Resolution(64, 32, 1);
        assert_eq!(r.y_bounds(), [-31.0, 0.0]);
    }

    #[test]
    fn test_px_iterator() {
        let r = Resolution(64, 32, 1);
        let px = r.points_from_data(&[0; 256]);
        for (_x, _y, colour) in px {
            assert_eq!(colour, Color::Black);
        }
    }

    // MonoTermDisplay tests
    #[test]
    fn test_display_size() {
        let mut d = MonoTermDisplay::new(64, 32).unwrap();
        assert_eq!(d.get_display_size_bytes(), 256);
    }

    #[test]
    #[should_panic]
    fn test_draw_rejects_wrong_data() {
        let mut d = MonoTermDisplay::new(64, 32).unwrap();
        let _ = d.draw(&[0; 257]);
    }

    #[test]
    #[ignore]
    // NB. figure out how to stop rendering during tests
    fn test_draw_accepts_test_card() -> Result<(), io::Error> {
        let mut d = MonoTermDisplay::new(64, 32).unwrap();
        d.draw(&CHIP8_TEST_CARD)
    }
}

/// this is a display test card suitable for CHIP8, for testing display routines
#[rustfmt::skip]
pub const CHIP8_TEST_CARD: [u8; 256] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // 00 XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|
    0x80, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x01, // 01 X                              |X                              |
    0x80, 0x00, 0x00, 0x03, 0xc2, 0x41, 0x55, 0x55, // 02 X                             X|XX    X  X     | X X X | X X X |
    0x81, 0xff, 0xff, 0xc5, 0xa2, 0x40, 0xaa, 0xa9, // 03 X      |XXXXXXX|XXXXXXX|XX   X |X X   X  X      X X X X X X X  |
    0x80, 0x00, 0x00, 0x09, 0x92, 0x41, 0x55, 0x55, // 04 X                           X  |X  X  X  X     | X X X | X X X |
    0x81, 0xff, 0xff, 0xc1, 0x82, 0x40, 0xaa, 0xa9, // 05 X      |XXXXXXX|XXXXXXX|XX     |X     X  X      X X X X X X X  |
    0xa0, 0x00, 0x00, 0x01, 0x83, 0xc1, 0x55, 0x55, // 06 X X                            |X     X|XX     | X X X | X X X |
    0xa1, 0xff, 0xff, 0xc1, 0x80, 0x00, 0xaa, 0xa9, // 07 X X    |XXXXXXX|XXXXXXX|XX     |X               X X X X X X X  |
    0xa0, 0x00, 0x00, 0x00, 0x00, 0x01, 0x55, 0x55, // 08 X X                                            | X X X | X X X |
    0xa1, 0xff, 0xff, 0xc0, 0x00, 0x00, 0xaa, 0xa9, // 09 X X    |XXXXXXX|XXXXXXX|XX                      X X X X X X X  |
    0xbc, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // 10 X XXXX                                                         |
    0x81, 0xff, 0xff, 0xc0, 0x00, 0x00, 0x00, 0x01, // 11 X      |XXXXXXX|XXXXXXX|XX                                     |
    0x88, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x11, // 12 X   X                          |X                          X   |
    0x91, 0xff, 0xff, 0xc1, 0x80, 0x00, 0x00, 0x09, // 13 X  X   |XXXXXXX|XXXXXXX|XX     |X                           X  |
    0xa0, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x05, // 14 X X                            |X                            X |
    0xff, 0x80, 0x00, 0x1f, 0xf8, 0x00, 0x01, 0xff, // 15 XXXXXXX|X                  XXXX|XXXXX                  |XXXXXXX|
    0xff, 0x80, 0x00, 0x1f, 0xf8, 0x00, 0x01, 0xff, // 16 XXXXXXX|X                  XXXX|XXXXX                  |XXXXXXX|
    0xa0, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x05, // 17 X X                            |X                            X |
    0x90, 0x00, 0x00, 0x01, 0x85, 0x55, 0x55, 0x09, // 18 X  X                           |X    X | X X X | X X X |    X  |
    0x88, 0x00, 0x00, 0x01, 0x85, 0x55, 0x55, 0x11, // 19 X   X                          |X    X | X X X | X X X |   X   |
    0x80, 0x00, 0x00, 0x00, 0x05, 0x55, 0x55, 0x01, // 20 X                                    X | X X X | X X X |       |
    0x80, 0x00, 0x00, 0x00, 0x05, 0x55, 0x55, 0x3d, // 21 X                                    X | X X X | X X X |  XXXX |
    0x95, 0x55, 0x40, 0x00, 0x05, 0x55, 0x55, 0x25, // 22 X  X X | X X X | X                   X | X X X | X X X |  X  X |
    0xaa, 0xaa, 0x80, 0x00, 0x05, 0x55, 0x55, 0x3d, // 23 X X X X X X X X X                    X | X X X | X X X |  XXXX |
    0x95, 0x55, 0x40, 0x01, 0x85, 0x55, 0x55, 0x29, // 24 X  X X | X X X | X             |X    X | X X X | X X X |  X X  |
    0xaa, 0xaa, 0x83, 0xc1, 0x85, 0x55, 0x55, 0x25, // 25 X X X X X X X X X     X|XX     |X    X | X X X | X X X |  X  X |
    0x95, 0x55, 0x41, 0x41, 0x85, 0x55, 0x55, 0x01, // 26 X  X X | X X X | X     | X     |X    X | X X X | X X X |       |
    0xaa, 0xaa, 0x81, 0x49, 0x95, 0x55, 0x55, 0x01, // 27 X X X X X X X X X      | X  X  |X  X X | X X X | X X X |       |
    0x95, 0x55, 0x41, 0x45, 0xa5, 0x55, 0x55, 0x01, // 28 X  X X | X X X | X     | X   X |X X  X | X X X | X X X |       |
    0xaa, 0xaa, 0x83, 0xc3, 0xc5, 0x55, 0x55, 0x01, // 29 X X X X X X X X X     X|XX    X|XX   X | X X X | X X X |       |
    0x80, 0x00, 0x00, 0x01, 0x80, 0x00, 0x00, 0x01, // 30 X                              |X                              |
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // 31 XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|XXXXXXX|
]; //                                                  .. 0......78......f0......78......f0......78......f0......78......f
