use std::env;

fn main() {
    // read env variables that were set in build script
    let uefi_path = env!("UEFI_PATH");

    println!("UEFI Path s{:?}", uefi_path);

    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    if let Some(x) = env::args().nth(1) {
        if x == "DEBUG" {
            cmd.arg("-s");
            cmd.arg("-S");
        };
    };
    // cmd.arg("-d").arg("int");
    cmd.arg("-debugcon").arg("stdio");
    cmd.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
    cmd.arg("-drive")
        .arg(format!("format=raw,file={uefi_path}"));

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
