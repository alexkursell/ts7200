pub mod interrupts;
pub mod vicmanager;

pub use interrupts::Interrupt;
pub use vicmanager::VicManager;

use crate::devices::{Device, Probe};
use crate::memory::{MemException::*, MemResult, Memory};

#[derive(Debug, Default)]
struct VectorEntry {
    source: u8,
    isr_addr: u32,
    enabled: bool,
}

/// VIC module
///
/// As described in section 6 of the EP93xx User's Guide
#[derive(Debug)]
pub struct Vic {
    label: &'static str,
    status: u32,          // Interrupts currently hardware asserted
    enabled: u32,         // Enabled interrupts
    select: u32,          // FIQ mode interrupts
    software_status: u32, // Software asserted interrupts

    default_isr: u32,

    vector_entries: [VectorEntry; 16],
}

impl Vic {
    /// Create a new Vic
    pub fn new(label: &'static str) -> Vic {
        Vic {
            label,
            status: 0,
            enabled: 0,
            select: 0,
            software_status: 0,
            default_isr: 0,
            vector_entries: Default::default(),
        }
    }

    fn rawstatus(&self) -> u32 {
        self.software_status | self.status
    }

    fn enabled_active_interrupts(&self) -> u32 {
        self.rawstatus() & self.enabled
    }

    /// Check if an IRQ should be requested
    pub fn irq(&self) -> bool {
        (self.enabled_active_interrupts() & !self.select) != 0
    }

    /// Check if an FIQ should be requested
    pub fn fiq(&self) -> bool {
        (self.enabled_active_interrupts() & self.select) != 0
    }

    fn isr_address(&self) -> u32 {
        if self.fiq() || !self.irq() {
            self.default_isr
        } else {
            let irqs = self.enabled_active_interrupts() & !self.select;
            self.vector_entries
                .iter()
                .find_map(|entry| {
                    if entry.enabled && (irqs & (1 << entry.source)) != 0 {
                        Some(entry.isr_addr)
                    } else {
                        None
                    }
                })
                .unwrap_or(self.default_isr)
        }
    }

    /// Request an interrupt from a hardware source
    pub fn assert_interrupt(&mut self, source: u8) {
        self.status |= 1 << source;
    }

    /// Clear an interrupt from a hardware source
    pub fn clear_interrupt(&mut self, source: u8) {
        self.status &= !(1 << source);
    }
}

impl Device for Vic {
    fn kind(&self) -> &'static str {
        "VIC"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn probe(&self, offset: u32) -> Probe<'_> {
        let reg = match offset {
            0x00 => "IRQStatus",
            0x04 => "FIQStatus",
            0x08 => "RawIntr",
            0x0c => "IntSelect",
            0x10 => "IntEnable",
            0x14 => "IntEnClear",
            0x18 => "SoftInt",
            0x1c => "SoftIntClear",
            0x20 => "Protection",
            0x30 => "VectAddr",
            0x34 => "DefVectAddr",
            0x100..=0x13c => "VectAddrX",
            0x200..=0x23c => "VectCntlX",
            0xfe0..=0xfe4 => "PeriphIDX",
            _ => return Probe::Unmapped,
        };
        Probe::Register(reg)
    }
}

impl Memory for Vic {
    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        match offset {
            0x00 => Ok(self.enabled_active_interrupts() & !self.select),
            0x04 => Ok(self.enabled_active_interrupts() & self.select),
            0x08 => Ok(self.rawstatus()),
            0x0c => Ok(self.select),
            0x10 => Ok(self.enabled),
            0x14 => Err(InvalidAccess),
            0x18 => Ok(self.software_status),
            0x1c => Err(InvalidAccess),
            // TODO: enforce that VIC Protection bit must be accessed in privileged mode
            0x20 => Err(StubRead(0)),
            0x30 => Ok(self.isr_address()),
            0x34 => Ok(self.default_isr),
            0x100..=0x13c => {
                let index = ((offset - 0x100) / 4) as usize;
                Ok(self.vector_entries[index].isr_addr)
            }
            0x200..=0x23c => {
                let index = ((offset - 0x200) / 4) as usize;
                let entry = &self.vector_entries[index];
                let result = (if entry.enabled { 0x20 } else { 0 }) + entry.source as u32;
                Ok(result)
            }
            // Next 4 values are hard-wired hardware identification values
            0xfe0 => Ok(0x90),
            0xfe4 => Ok(0x11),
            0xfe8 => Ok(0x04),
            0xfec => Ok(0x00),
            _ => Err(Unexpected),
        }
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        match offset {
            0x00 => Err(InvalidAccess),
            0x04 => Err(InvalidAccess),
            0x08 => Err(InvalidAccess),
            0x0c => Ok(self.select = val),
            0x10 => Ok(self.enabled = val),
            0x14 => Ok(self.enabled &= !val),
            0x18 => Ok(self.software_status |= val),
            0x1c => Ok(self.software_status &= !val),
            // TODO: enforce that VIC Protection bit must be accessed in privileged mode
            0x20 => Err(StubWrite),
            // Writing to this signals to the Vic that the interrupt has been serviced.
            // We don't implement the behavior that cares about that for now, so no-op.
            0x30 => Ok(()),
            0x34 => Ok(self.default_isr = val),
            0x100..=0x13c => {
                let index = ((offset - 0x100) / 4) as usize;
                Ok(self.vector_entries[index].isr_addr = val)
            }
            0x200..=0x23c => {
                let index = ((offset - 0x200) / 4) as usize;
                let entry = &mut self.vector_entries[index];
                entry.enabled = (val & 0x20) != 0;
                entry.source = (val & 0x1f) as u8;

                Ok(())
            }
            0xfe0 => Err(InvalidAccess),
            0xfe4 => Err(InvalidAccess),
            0xfe8 => Err(InvalidAccess),
            0xfec => Err(InvalidAccess),
            _ => Err(Unexpected),
        }
    }
}
