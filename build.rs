use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let root_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());

    // Compile init
    let init_path = root_dir.join("init");
    println!("cargo::rerun-if-changed={}", init_path.display());

    let mut make_cmd = std::process::Command::new("make");
    make_cmd.current_dir(&init_path);
    make_cmd.spawn().unwrap().wait().unwrap();

    let init_out = init_path.clone().join("init");

    // Compile kernel
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let uefi_path = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .set_ramdisk(init_out.as_path())
        .create_disk_image(&uefi_path)
        .unwrap();

    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
}
