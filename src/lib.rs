///
/// ## Design
///
/// * authentic timing to COSMAC VIP as much as possible
/// * map to machine cycle (not 1.76064 MHz clock)
/// * abstract display so can plug alternatives; starting with TUI in-console
/// * anticipate emulation of RCA CDP 1802 itself
/// * CHIP-8 instructions will run as fast as possible then sleep, to match
///   timings; so not quite authentic
///
/// Enums to represent:
///
/// * memory map
///    - should allow for RAM, ROM and DMA in the future
///    - need a means of initialising from an external file or whatever
/// * instruction set
/// * the interpreter itself
///    - pub .cycle() -> n -- move on n machine cycle(s)
///    - pub .interrupt(reason) -- interrupt for reason
///    - need to maintain simple state machine such that we can .cycle() and
///      keep proper timings for fetch/decode/execute
///    - state machine also needs to wait for interrupts. whilst it's doing
///      this .cycle() does nothing and returns 1
/// * some config (e.g. CHIP-8 vs. SUPER-CHIP)
/// * the environment
///    - sets everything up; runs the main loop
///    - maintains a queue of interrupt handlers, ordered by next to fire
/// * display, with trait for rendering
///    - provide an interface such that the interpreter doesn't need to know
///      how the display works
/// * input device, with trait for reading key-presses
/// * audio device, with trait for making beeps
///
/// Model
///
/// Environment
///  |-- display, input, audio, config, memory(config)
///  |-- interpreter(display, input, audio, memory, config)
///  |    |-- instruction set(config)
///  |    `-- set up machine state(config)
///  `-- main loop
///       |   // this logic gets the timing mostly right; altho the interrupt always
///       |   // happens after the CHIP-8 instruction is processed. i.e. wallclock
///       |   // timing will look good, but some things might happen too quickly anyway
///       |-- new_cycles = interpreter.cycle();
///       |-- while interrupt_queue.top().would_interrupt(cycles + new_cycles) {
///       |     sleep(some_proportion_of(new_cycles)); new_cycles -= that proportion;
///       |     interpreter.interrupt(REASON);
///       |     interrupt_queue.insert(interrupt_queue.pop())
///       |   }
///       `-- sleep(new_cycles * 4.54us)
mod interpreter;
pub mod display;
