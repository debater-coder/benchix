# benchix

Benchix is a Unix-like operating system that can run on real hardware (tested on Dell Inspiron 13 5330).
It runs on the x86-64 platform, and boots from UEFI.

**Read the blog post for more information**: [Writing a Unix-like Operating System: Part 1 | Hamzah Ahmed](https://hamzah.syedahmed.net/posts/benchix-osdev)

## Debugging

TODO automate this.

```
add-symbol-file /home/hamzah/benchix/target/x86_64-unknown-none/debug/build/kernel/96447d99be3dc9b3/artifact/bin/kernel -o 0xffff800000000000
```

To load symbol files.
