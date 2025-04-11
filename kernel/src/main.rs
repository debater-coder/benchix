#![feature(abi_x86_interrupt, new_zeroed_alloc)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use core::fmt::Write;
use core::str;
use filesystem::devfs::Devfs;
use filesystem::initrd::Initrd;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use lapic::Lapic;
use memory::PhysicalMemoryManager;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags};

mod console;
mod filesystem;
mod gdt;
mod interrupts;
mod lapic;
mod memory;

use crate::console::Console;
use alloc::{fmt, vec};
use bootloader_api::config::Mapping;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use bootloader_api::BootloaderConfig;
use core::panic::PanicInfo;
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};
use x86_64::instructions::hlt;

struct PanicConsole {
    x: usize,
    y: usize,
    frame_buffer: &'static mut FrameBuffer,
}

impl PanicConsole {
    fn new_line(x: &mut usize, y: &mut usize, info: FrameBufferInfo) {
        if *y < info.height - 32 {
            *y += 32;
            *x = 0;
        } else {
            loop {
                hlt();
            }
        }
    }
}

impl Write for PanicConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let info = self.frame_buffer.info().clone();
        let buffer = self.frame_buffer.buffer_mut();

        for byte in s.as_bytes() {
            match byte {
                b'\n' => {
                    Self::new_line(&mut self.x, &mut self.y, info);
                }
                _ => {
                    let width = get_raster_width(FontWeight::Regular, RasterHeight::Size32);
                    if self.x + width >= info.width {
                        Self::new_line(&mut self.x, &mut self.y, info);
                    }

                    let raster =
                        get_raster(*byte as char, FontWeight::Regular, RasterHeight::Size32)
                            .unwrap_or_else(|| loop {
                                hlt()
                            })
                            .raster();

                    for (row_i, row) in raster.iter().enumerate() {
                        for (col_i, pixel) in row.iter().enumerate() {
                            let y = self.y + row_i;
                            let x = self.x + col_i;

                            let base = (y * info.stride + x) * info.bytes_per_pixel;
                            buffer[base] = *pixel;
                            buffer[base + 1] = *pixel;
                            buffer[base + 2] = *pixel;
                        }
                    }
                    self.x += width;
                }
            }
        }

        Ok(())
    }
}

static mut PANIC_FRAMEBUFFER: Option<*mut FrameBuffer> = None;
/// This function is called on panic.
/// On kernel panic, it is best to use as little existing infrastructure as possible as it may be
/// corrupted. This panic function is responsible for showing the panic info which was passed to it.
/// In order to avoid relying on the filesystem (to access the console), the panic handler instead
/// reinitialises the console from the framebuffer. This would normally be a violation of no mutable
/// aliasing rules, so to remain safe the panic handler is responsible for terminating all other
/// code running in the system, so it can have complete control without any rogue threads interfering.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    debug_println!("panicked: {}", info);
    if let Some(framebuffer) = unsafe { PANIC_FRAMEBUFFER } {
        let framebuffer = unsafe { &mut *framebuffer };

        {
            let (info, buffer) = (framebuffer.info().clone(), framebuffer.buffer_mut());

            for x in 0..info.width {
                for y in 0..info.height {
                    let base = (y * info.stride + x) * info.bytes_per_pixel;
                    buffer[base] = 0;
                    buffer[base + 1] = 0;
                    buffer[base + 2] = 0;
                }
            }
        }

        let mut console = PanicConsole {
            x: 0,
            y: 0,
            frame_buffer: framebuffer,
        };

        let _ = write!(&mut console, "panicked: {}", info);
    }

    loop {}
}

pub const HEAP_START: u64 = 0x_ffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_f700_0000_0000;
pub const KERNEL_STACK_SIZE: u64 = 80 * 1024; // 80 Kb
pub const LAPIC_START_VIRT: u64 = 0xffff_8fff_ffff_0000;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.kernel_stack_size = KERNEL_STACK_SIZE;
    config.mappings.kernel_stack = Mapping::FixedAddress(KERNEL_STACK_START);
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0xffff_e000_0000_0000)); // 16 TiB of RAM ought to be enough for anybody
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);
    config.mappings.dynamic_range_end = Some(0xffff_8fff_fffe_ffff);
    config
};

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    unsafe { *&raw mut PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }

    gdt::init();
    interrupts::init_idt();

    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Expected recursive index");

    let (mut mapper, mut pmm) = unsafe { memory::init(physical_offset, &boot_info.memory_regions) };

    let mut console = Console::new(framebuffer);

    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };
    let mut lapic = unsafe { Lapic::new(&mut mapper, &mut pmm, 0xff) };
    lapic.configure_timer(0x31, 0xffffff, lapic::TimerDivideConfig::DivideBy16);
    x86_64::instructions::interrupts::enable();
    boot_println!(&mut console, "Boot complete!");

    let mut vfs = VirtualFileSystem::new();
    let devfs = Devfs::new(console, 1);
    let initrd = Initrd::from_files(
        2,
        vec![
            ("hello_world.txt", "Hello from initrd!"),
            (
                "Othello.txt",
                r"Othello, the Moore of Venice
ACT I

SCENE I. Venice. A street.

Enter RODERIGO and IAGO
RODERIGO
Tush! never tell me; I take it much unkindly
That thou, Iago, who hast had my purse
As if the strings were thine, shouldst know of this.
IAGO
'Sblood, but you will not hear me:
If ever I did dream of such a matter, Abhor me.
RODERIGO
Thou told'st me thou didst hold him in thy hate.
IAGO
Despise me, if I do not. Three great ones of the city,
In personal suit to make me his lieutenant,
Off-capp'd to him: and, by the faith of man,
I know my price, I am worth no worse a place:
But he; as loving his own pride and purposes,
Evades them, with a bombast circumstance
Horribly stuff'd with epithets of war;
And, in conclusion,
Nonsuits my mediators; for, 'Certes,' says he,
'I have already chose my officer.'
And what was he?
Forsooth, a great arithmetician,
One Michael Cassio, a Florentine,
A fellow almost damn'd in a fair wife;
That never set a squadron in the field,
Nor the division of a battle knows
More than a spinster; unless the bookish theoric,
Wherein the toged consuls can propose
As masterly as he: mere prattle, without practise,
Is all his soldiership. But he, sir, had the election:
And I, of whom his eyes had seen the proof
At Rhodes, at Cyprus and on other grounds
Christian and heathen, must be be-lee'd and calm'd
By debitor and creditor: this counter-caster,
He, in good time, must his lieutenant be,
And I--God bless the mark!--his Moorship's ancient.
RODERIGO
By heaven, I rather would have been his hangman.
IAGO
Why, there's no remedy; 'tis the curse of service,
Preferment goes by letter and affection,
And not by old gradation, where each second
Stood heir to the first. Now, sir, be judge yourself,
Whether I in any just term am affined
To love the Moor.
RODERIGO
I would not follow him then.
IAGO
O, sir, content you;
I follow him to serve my turn upon him:
We cannot all be masters, nor all masters
Cannot be truly follow'd. You shall mark
Many a duteous and knee-crooking knave,
That, doting on his own obsequious bondage,
Wears out his time, much like his master's ass,
For nought but provender, and when he's old, cashier'd:
Whip me such honest knaves. Others there are
Who, trimm'd in forms and visages of duty,
Keep yet their hearts attending on themselves,
And, throwing but shows of service on their lords,
Do well thrive by them and when they have lined
their coats
Do themselves homage: these fellows have some soul;
And such a one do I profess myself. For, sir,
It is as sure as you are Roderigo,
Were I the Moor, I would not be Iago:
In following him, I follow but myself;
Heaven is my judge, not I for love and duty,
But seeming so, for my peculiar end:
For when my outward action doth demonstrate
The native act and figure of my heart
In compliment extern, 'tis not long after
But I will wear my heart upon my sleeve
For daws to peck at: I am not what I am.
RODERIGO
What a full fortune does the thicklips owe
If he can carry't thus!
IAGO
Call up her father,
Rouse him: make after him, poison his delight,
Proclaim him in the streets; incense her kinsmen,
And, though he in a fertile climate dwell,
Plague him with flies: though that his joy be joy,
Yet throw such changes of vexation on't,
As it may lose some colour.
RODERIGO
Here is her father's house; I'll call aloud.
IAGO
Do, with like timorous accent and dire yell
As when, by night and negligence, the fire
Is spied in populous cities.
RODERIGO
What, ho, Brabantio! Signior Brabantio, ho!
IAGO
Awake! what, ho, Brabantio! thieves! thieves! thieves!
Look to your house, your daughter and your bags!
Thieves! thieves!
BRABANTIO appears above, at a window

BRABANTIO
What is the reason of this terrible summons?
What is the matter there?
RODERIGO
Signior, is all your family within?
IAGO
Are your doors lock'd?
BRABANTIO
Why, wherefore ask you this?
IAGO
'Zounds, sir, you're robb'd; for shame, put on
your gown;
Your heart is burst, you have lost half your soul;
Even now, now, very now, an old black ram
Is topping your white ewe. Arise, arise;
Awake the snorting citizens with the bell,
Or else the devil will make a grandsire of you:
Arise, I say.
BRABANTIO
What, have you lost your wits?
RODERIGO
Most reverend signior, do you know my voice?
BRABANTIO
Not I	what are you?
RODERIGO
My name is Roderigo.
BRABANTIO
The worser welcome:
I have charged thee not to haunt about my doors:
In honest plainness thou hast heard me say
My daughter is not for thee; and now, in madness,
Being full of supper and distempering draughts,
Upon malicious bravery, dost thou come
To start my quiet.
RODERIGO
Sir, sir, sir,--
BRABANTIO
But thou must needs be sure
My spirit and my place have in them power
To make this bitter to thee.
RODERIGO
Patience, good sir.
BRABANTIO
What tell'st thou me of robbing? this is Venice;
My house is not a grange.
RODERIGO
Most grave Brabantio,
In simple and pure soul I come to you.
IAGO
'Zounds, sir, you are one of those that will not
serve God, if the devil bid you. Because we come to
do you service and you think we are ruffians, you'll
have your daughter covered with a Barbary horse;
you'll have your nephews neigh to you; you'll have
coursers for cousins and gennets for germans.
BRABANTIO
What profane wretch art thou?
IAGO
I am one, sir, that comes to tell you your daughter
and the Moor are now making the beast with two backs.
BRABANTIO
Thou art a villain.
IAGO
You are--a senator.
BRABANTIO
This thou shalt answer; I know thee, Roderigo.
RODERIGO
Sir, I will answer any thing. But, I beseech you,
If't be your pleasure and most wise consent,
As partly I find it is, that your fair daughter,
At this odd-even and dull watch o' the night,
Transported, with no worse nor better guard
But with a knave of common hire, a gondolier,
To the gross clasps of a lascivious Moor--
If this be known to you and your allowance,
We then have done you bold and saucy wrongs;
But if you know not this, my manners tell me
We have your wrong rebuke. Do not believe
That, from the sense of all civility,
I thus would play and trifle with your reverence:
Your daughter, if you have not given her leave,
I say again, hath made a gross revolt;
Tying her duty, beauty, wit and fortunes
In an extravagant and wheeling stranger
Of here and every where. Straight satisfy yourself:
If she be in her chamber or your house,
Let loose on me the justice of the state
For thus deluding you.
BRABANTIO
Strike on the tinder, ho!
Give me a taper! call up all my people!
This accident is not unlike my dream:
Belief of it oppresses me already.
Light, I say! light!
Exit above

IAGO
Farewell; for I must leave you:
It seems not meet, nor wholesome to my place,
To be produced--as, if I stay, I shall--
Against the Moor: for, I do know, the state,
However this may gall him with some cheque,
Cannot with safety cast him, for he's embark'd
With such loud reason to the Cyprus wars,
Which even now stand in act, that, for their souls,
Another of his fathom they have none,
To lead their business: in which regard,
Though I do hate him as I do hell-pains.
Yet, for necessity of present life,
I must show out a flag and sign of love,
Which is indeed but sign. That you shall surely find him,
Lead to the Sagittary the raised search;
And there will I be with him. So, farewell.
Exit
    ",
            ),
        ],
    );

    vfs.mount(1, Box::new(devfs), "dev", 0);
    vfs.mount(2, Box::new(initrd), "init", 0);

    debug_println!("/ entries:");
    for entry in vfs.readdir(vfs.root.clone()).unwrap() {
        debug_println!(
            "name: {}, (dev, inode): ({}, {})",
            entry.name,
            entry.dev,
            entry.inode
        );
    }

    debug_println!("/dev entries:");
    for entry in vfs
        .readdir(vfs.traverse_fs(vfs.root.clone(), "/dev").unwrap())
        .unwrap()
    {
        debug_println!(
            "name: {}, (dev, inode): ({}, {})",
            entry.name,
            entry.dev,
            entry.inode
        );
    }

    debug_println!("/init entries:");
    for entry in vfs
        .readdir(vfs.traverse_fs(vfs.root.clone(), "/init").unwrap())
        .unwrap()
    {
        debug_println!(
            "name: {}, (dev, inode): ({}, {})",
            entry.name,
            entry.dev,
            entry.inode
        );
        let inode = vfs.inode(entry.dev, entry.inode).unwrap();
        let mut contents = vec![0u8; inode.size];
        vfs.read(inode, 0, contents.as_mut());

        debug_println!("{}", str::from_utf8(contents.as_slice()).unwrap());
        debug_println!();
    }

    vfs.write(
        vfs.traverse_fs(vfs.root.clone(), "/dev/console").unwrap(),
        0,
        &"Hello, from /dev/console".as_bytes(),
    )
    .unwrap();

    // unsafe {
    //     // Allocates user code
    //     let user_addr = VirtAddr::new(0x400000);
    //     allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(user_addr));
    //     user_addr.as_mut_ptr::<u16>().write(0xFEEB); // Infinite loop

    //     // Allocates user stack
    //     let stack_addr = VirtAddr::new(0x0000_7fff_ffff_0000);
    //     allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(stack_addr));

    //     x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
    //     asm!(
    //         "mov rsp, 0x00007fffffffffff", // Stacks grow downwards
    //         "mov r11, 0x0202",             // Bit 9 is set, thus interrupts are enabled
    //         "mov rcx, 0x400000",
    //         "sysretq"
    //     );
    // }

    loop {
        hlt();
    }
}

unsafe fn allocate_user_page(
    mapper: &mut OffsetPageTable,
    pmm: &mut PhysicalMemoryManager,
    page: Page,
) {
    mapper
        .map_to(
            page,
            pmm.allocate_frame().expect("Could not allocate frame"),
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            pmm,
        )
        .unwrap()
        .flush();
}
