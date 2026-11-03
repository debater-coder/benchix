# benchix

Benchix is a Unix-like operating system that can run on real hardware (tested on
Dell Inspiron 13 5330). It runs on the x86-64 platform, and boots from UEFI.

**Read the blog post for more information**: [Writing a Unix-like Operating
System: Part 1 | Hamzah Ahmed](https://hamzah.syedahmed.net/posts/benchix-osdev)

## Debugging

Enter the dev-shell with `nix develop`. Then run `cargo run -- -d`, and then
`benchix-gdb <KERNEL_ELF>` where `KERNEL_ELF` is the path printed by the runner.
The script will automatically connect to QEMU at `localhost:1234` by default. A
custom debug target can be set as the second argument.
