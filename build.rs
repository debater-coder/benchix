use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    // Compile kernel
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let uefi_path = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .set_ramdisk(&PathBuf::from(std::env::var_os("RAMDISK_PATH").unwrap()))
        .create_disk_image(&uefi_path)
        .unwrap();

    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
    println!("cargo:rustc-env=ELF_PATH={}", kernel.display());

    if let Ok(dest) = std::env::var("KERNEL_ARTIFACT_DIR") {
        let dest = std::path::PathBuf::from(dest);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::copy(&kernel, dest.join("kernel.elf")).unwrap();
        std::fs::copy(&uefi_path, dest.join("kernel.img")).unwrap();
    }
}
