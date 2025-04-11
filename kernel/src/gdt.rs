use lazy_static::lazy_static;
use x86_64::instructions::segmentation::Segment;
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::registers::control::{Efer, EferFlags};
use x86_64::registers::model_specific::Star;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::{registers, VirtAddr};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    data_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
}

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(unsafe { &raw const STACK });
            let stack_end = stack_start + STACK_SIZE as u64;

            stack_end // stacks grow downwards
        };
        tss
    };
}

// There is one GDT for all CPUs
lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();

        // Intel manual vol 3 3.4.2: A segment selector is a 16-bit identifier for a segment (see Figure 3-6). It does not point directly to the segment,
        // but instead points to the segment descriptor that defines the segment.
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        // One per CPU
        let tss_selector = gdt.append(Descriptor::tss_segment(&TSS)); // The TSS should still be able to be mutated after this
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        let user_code_selector = gdt.append(Descriptor::user_code_segment());

        (
            gdt,
            Selectors {
                code_selector,
                tss_selector,
                data_selector,
                user_code_selector,
                user_data_selector
            },
        )
    };
}

pub fn init() {
    GDT.0.load();

    unsafe {
        CS::set_reg(GDT.1.code_selector);
        load_tss(GDT.1.tss_selector);

        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        FS::set_reg(GDT.1.data_selector);
        GS::set_reg(GDT.1.data_selector);
        SS::set_reg(GDT.1.data_selector);

        // Prepare for usermode
        Efer::write(Efer::read() | EferFlags::SYSTEM_CALL_EXTENSIONS);
        Star::write(
            GDT.1.user_code_selector,
            GDT.1.user_data_selector,
            GDT.1.code_selector,
            GDT.1.data_selector,
        )
        .unwrap();
        // TODO: Write sycall RIP to LSTAR
    }
}
