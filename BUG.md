panicked: panicked at /home/hamzah/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/vec/mod.rs:1671:18:
unsafe precondition(s) violated: slice::from_raw_parts requires the pointer to be aligned and non-null, and the total size of the slice not to exceed `isize::MAX`

This indicates a bug in the program. This Undefined Behavior check is optional, and cannot be relied on for safety.
