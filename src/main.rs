use std::{env, path::PathBuf};

use clap::Parser;

#[derive(Parser)]
#[command(name = "Benchix Runner")]
struct Cli {
    /// Connect to GDB
    #[arg(short, long)]
    debug: bool,
    /// Send trace events to /tmp/debugcon.sock
    #[arg(short, long)]
    trace: bool,
}

fn main() {
    let cli = Cli::parse();

    // read env variables that were set in build script
    let uefi_path = env::var("UEFI_PATH").unwrap();
    let elf_path = env::var("ELF_PATH").unwrap();

    println!("UEFI Path {:?}", uefi_path);
    println!("Kernel ELF Path {:?}", elf_path);

    let mut cmd = std::process::Command::new("qemu-system-x86_64");

    if cli.debug {
        cmd.arg("-s");
        cmd.arg("-S");
    };

    if cli.trace {
        cmd.arg("-chardev")
            .arg("socket,id=debugsock,path=/tmp/debugcon.sock,server=on,wait=on");
        cmd.arg("-device")
            .arg("isa-debugcon,iobase=0xe9,chardev=debugsock");
    }

    cmd.arg("-drive")
        .arg(format!("format=raw,file={uefi_path},snapshot=on"));

    cmd.arg("-bios")
        .arg(PathBuf::from(env::var_os("OVMF_FD").unwrap()));

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
