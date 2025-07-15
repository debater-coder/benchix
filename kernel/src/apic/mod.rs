use acpi::InterruptModel;
use alloc::alloc::Global;
use ioapic::IoApic;
use lapic::Lapic;
use x86_64::{registers::model_specific::Msr, structures::paging::OffsetPageTable};

mod ioapic;
pub mod lapic;

#[allow(dead_code)]
enum IsaIrq {
    PitTimer = 0,
    Keyboard = 1,
    Com2 = 3,
    Com1 = 4,
    Lpt2 = 5,
    FloppyDisk = 6,
    Isa7 = 7,
    Rtc = 8,
    Isa9 = 9,
    Isa10 = 10,
    Isa11 = 11,
    Mouse = 12,
    Isa13 = 13,
    PrimaryAta = 14,
    SecondaryAta = 15,
}

/// Interface for the ancient 8259 PIC.
#[allow(dead_code)]
mod pic {
    use x86_64::instructions::port::Port;

    const ICW1_ICW4: u8 = 0x01; /* Indicates that ICW4 will be present */
    const ICW1_SINGLE: u8 = 0x02; /* Single (cascade) mode */
    const ICW1_INTERVAL4: u8 = 0x04; /* Call address interval 4 (8) */
    const ICW1_LEVEL: u8 = 0x08; /* Level triggered (edge) mode */
    const ICW1_INIT: u8 = 0x10; /* Initialization - required! */

    const ICW4_8086: u8 = 0x01; /* 8086/88 (MCS-80/85) mode */
    const ICW4_AUTO: u8 = 0x02; /* Auto (normal) EOI */
    const ICW4_BUF_SLAVE: u8 = 0x08; /* Buffered mode/slave */
    const ICW4_BUF_MASTER: u8 = 0x0C; /* Buffered mode/master */
    const ICW4_SFNM: u8 = 0x10; /* Special fully nested (not) */

    const CASCADE_IRQ: u8 = 2;

    struct Pic {
        command: Port<u8>,
        data: Port<u8>,
    }

    /// Remaps PICs (to avoid spurious interrupts colliding with CPU exceptions) and mask all IRQs
    pub fn initialise(offset1: u8, offset2: u8) {
        let mut pic1 = Pic {
            command: Port::new(0x20),
            data: Port::new(0x21),
        };
        let mut pic2 = Pic {
            command: Port::new(0xA0),
            data: Port::new(0xA1),
        };

        let mut wait = Port::<u8>::new(0x80);

        // https://wiki.osdev.org/8259_PIC#Disabling
        unsafe {
            pic1.command.write(ICW1_INIT | ICW1_ICW4); // starts the initialization sequence (in cascade mode)
            wait.write(0);
            pic2.command.write(ICW1_INIT | ICW1_ICW4);
            wait.write(0);
            pic1.data.write(offset1); // ICW2: Master PIC vector offset
            wait.write(0);
            pic2.data.write(offset2); // ICW2: Slave PIC vector offset
            wait.write(0);
            pic1.data.write(1 << CASCADE_IRQ); // ICW3: tell Master PIC that there is a slave PIC at IRQ2
            wait.write(0);
            pic2.data.write(2); // ICW3: tell Slave PIC its cascade identity (0000 0010)
            wait.write(0);

            pic1.data.write(ICW4_8086); // ICW4: have the PICs use 8086 mode (and not 8080 mode)
            wait.write(0);
            pic2.data.write(ICW4_8086);
            wait.write(0);

            // Mask all interrupts
            pic1.data.write(0xff);
            pic2.data.write(0xff);
        }
    }

    pub struct IMCR {
        selector_port: Port<u8>,
        value_port: Port<u8>,
    }

    impl IMCR {
        pub fn new() -> Self {
            IMCR {
                selector_port: Port::new(0x22),
                value_port: Port::new(0x23),
            }
        }

        /// See: https://zygomatic.sourceforge.net/devref/group__arch__ia32__apic.html
        pub fn enable_symmetric_io_mode(&mut self) {
            unsafe {
                self.selector_port.write(0x70u8); // select IMCR
                self.value_port.write(0x01u8); // force NMI and INTR signals through the APIC}
            }
        }
    }
}

/// See: https://blog.wesleyac.com/posts/ioapic-interrupts
/// Also see: https://github.com/debater-coder/bench2/blob/7e1141f24de42d7e7cf3f0ad7e0425e3ec517714/kernel/src/io/drivers/apic/mod.rs#L105
pub fn enable(mapper: &mut OffsetPageTable<'static>, interrupt_model: &InterruptModel<Global>) {
    // Step 1. Disable PIC
    pic::initialise(0x20, 0x28);

    // Step 2. Set IMCR
    let mut imcr = pic::IMCR::new();
    imcr.enable_symmetric_io_mode();

    // Step 3. Configure LAPIC Spurious Interrupt Vector
    let mut lapic = unsafe { Lapic::new(mapper, 0xff) };

    // Step 4: read all of the Interrupt Source Override entries - if the IRQ source of any of them is 1 (Keyboard) use that in IOREDTBL
    let (ioapics, interrupt_source_overrides) = match interrupt_model {
        InterruptModel::Apic(apic_info) => {
            (&apic_info.io_apics, &apic_info.interrupt_source_overrides)
        }
        _ => {
            panic!("interrupt model is not apic")
        }
    };

    let ioapic = &ioapics[0];
    let keyboard_gsi = interrupt_source_overrides
        .iter()
        .filter_map(|interrupt_source_override| {
            if interrupt_source_override.isa_source == (IsaIrq::Keyboard as u8) {
                Some(interrupt_source_override.global_system_interrupt)
            } else {
                None
            }
        })
        .next()
        .unwrap_or(ioapic.global_system_interrupt_base + (IsaIrq::Keyboard as u32)); // A sensible default is that it is connected to the IOAPIC pin corresponding to its usual PIC pin

    if keyboard_gsi < ioapic.global_system_interrupt_base {
        panic!("No IOAPIC connected to keyboard");
    }

    let gsi_base = ioapic.global_system_interrupt_base;

    // Step 5: Configure the IOREDTBL entry in registers 0x12 and 0x13 (unless you need to use a different one, per the above step)
    let mut ioapic = IoApic::new(&mut *mapper, ioapic);
    ioapic.set_ioredtbl((keyboard_gsi - gsi_base) as u8, 0x41, lapic.lapic_id());

    // Step 6. Enable the APIC
    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };

    // Configuring timer interrupts on 0x31
    lapic.configure_timer(0x31, 1_000_000, lapic::TimerDivideConfig::DivideBy16);
}
