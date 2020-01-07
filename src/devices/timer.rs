use std::time::Instant;

use crate::memory::{MemResult, MemResultExt, Memory};

#[derive(Clone, Copy, Debug)]
enum Mode {
    FreeRunning = 0,
    Periodic = 1,
}

#[derive(Clone, Copy, Debug)]
enum Clock {
    Khz2 = 0,
    Khz508 = 1,
}

/// Timer module
///
/// As described in section 18
/// https://www.student.cs.uwaterloo.ca/~cs452/F19/docs/ep93xx-user-guide.pdf
pub struct Timer {
    label: &'static str,
    // registers
    loadval: Option<u32>,
    val: u32,
    enabled: bool,
    mode: Mode,
    clksel: Clock,
    // implementation details
    wrapmask: u32, // 0x0000FFFF for 16 bit timers, 0xFFFFFFFF for 32 bit timers
    last_time: Instant,
    microticks: u32,
}

impl std::fmt::Debug for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timer").finish()
    }
}

impl Timer {
    /// Create a new Timer
    pub fn new(label: &'static str, bits: usize) -> Timer {
        Timer {
            label,
            loadval: None,
            val: 0,
            enabled: false,
            mode: Mode::FreeRunning,
            clksel: Clock::Khz2,
            wrapmask: ((1u64 << bits) - 1) as u32,
            last_time: Instant::now(),
            microticks: 0,
        }
    }

    /// Lazily update the registers on read / write.
    fn update_regs(&mut self) {
        // calculate the time delta
        let now = Instant::now();
        let dt = now.duration_since(self.last_time).as_nanos() as u64;
        self.last_time = now;

        if !self.enabled {
            return;
        }

        let khz = match self.clksel {
            Clock::Khz2 => 2,
            Clock::Khz508 => 508,
        };

        // calculate number of ticks the timer should decrement by
        let microticks = dt * khz + self.microticks as u64;
        // FIXME: rounding down probably ain't the ideal behavior...
        let ticks = (microticks / 1_000_000) as u32;
        self.microticks = (microticks % 1_000_000) as u32;
        let ticks = ticks as u32;

        match self.mode {
            Mode::FreeRunning => {
                self.val = self.val.wrapping_sub(ticks) & self.wrapmask;
            }
            // XXX: double check this code...
            Mode::Periodic => {
                if self.val >= ticks {
                    self.val -= ticks;
                } else {
                    let loadval = match self.loadval {
                        Some(v) => v,
                        None => panic!("trying to use unset load value with {}", self.label),
                    };
                    let remaining_ticks = ticks - self.val;
                    self.val = loadval - remaining_ticks;
                }
            }
        }
    }
}

impl Memory for Timer {
    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn device(&self) -> &'static str {
        "Timer"
    }

    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        self.update_regs();

        match offset {
            0x00 => Ok(match self.loadval {
                Some(v) => v,
                None => panic!("tried to read {} Load before it's been set it", self.label),
            }),
            0x04 => Ok(self.val),
            0x08 => {
                let val = ((self.clksel as u32) << 3)
                    | ((self.mode as u32) << 6)
                    | ((self.enabled as u32) << 7);
                Ok(val)
            }
            // TODO: implement timer interrupts
            0x0C => crate::mem_unimpl!("CLR_REG"),
            _ => crate::mem_unexpected!(),
        }
        .mem_ctx(offset, self)
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        self.update_regs();

        match offset {
            0x00 => {
                // "The Load register should not be written after the Timer is enabled because
                // this causes the Timer Value register to be updated with an undetermined
                // value."
                if self.enabled {
                    panic!("tried to write to {} Load while the timer is enabled");
                }

                let val = val & self.wrapmask;
                self.loadval = Some(val);
                // "The Timer Value register is updated with the Timer Load value as soon as the
                // Timer Load register is written"
                self.val = val;
                Ok(())
            }
            0x04 => {
                // TODO: add warning about writing to registers that _shouldn't_ be written to,
                // instead of this hard panic
                panic!("tried to write value to Write-only Timer register");
            }
            0x08 => {
                self.clksel = match val & (1 << 3) != 0 {
                    true => Clock::Khz508,
                    false => Clock::Khz2,
                };
                self.mode = match val & (1 << 6) != 0 {
                    true => Mode::Periodic,
                    false => Mode::FreeRunning,
                };
                let previous_enabled = self.enabled;
                self.enabled = val & (1 << 7) != 0;

                if self.enabled && !previous_enabled {
                    self.microticks = 0;
                }
                if !self.enabled {
                    self.loadval = None;
                }

                Ok(())
            }
            // TODO: implement timer interrupts
            0x0C => crate::mem_unimpl!("CLR_REG"),
            _ => crate::mem_unexpected!(),
        }
        .mem_ctx(offset, self)
    }
}
