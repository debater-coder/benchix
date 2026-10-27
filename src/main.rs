use std::{env, path::PathBuf};

fn main() {
    // read env variables that were set in build script
    let uefi_path = env::var("UEFI_PATH").unwrap();
    let elf_path = env::var("ELF_PATH").unwrap();

    println!("UEFI Path s{:?}", uefi_path);
    println!("Kernel ELF Path s{:?}", elf_path);

    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    if let Some(x) = env::args().nth(1) {
        if x == "DEBUG" {
            cmd.arg("-s");
            cmd.arg("-S");
        };
    };
    // cmd.arg("-d").arg("int");
    cmd.arg("-debugcon").arg("stdio");
    cmd.arg("-bios")
        .arg(PathBuf::from(env::var_os("OVMF_FD").unwrap()));
    cmd.arg("-drive")
        .arg(format!("format=raw,file={uefi_path},snapshot=on"));

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
