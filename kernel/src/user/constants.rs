/// From: include/uapi/asm-generic/ in linux kernel
/// See also: https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/9388606854/artifacts/raw/x86-64-ABI/abi.pdf
/// A range between -4095 and -1 indicates an error, so these constants have type i64

// errno-base.h
pub const EPERM: i64 = 1; /* Operation not permitted */
pub const ENOENT: i64 = 2; /* No such file or directory */
pub const ESRCH: i64 = 3; /* No such process */
pub const EINTR: i64 = 4; /* Interrupted system call */
pub const EIO: i64 = 5; /* I/O error */
pub const ENXIO: i64 = 6; /* No such device or address */
pub const E2BIG: i64 = 7; /* Argument list too long */
pub const ENOEXEC: i64 = 8; /* Exec format error */
pub const EBADF: i64 = 9; /* Bad file number */
pub const ECHILD: i64 = 10; /* No child processes */
pub const EAGAIN: i64 = 11; /* Try again */
pub const ENOMEM: i64 = 12; /* Out of memory */
pub const EACCES: i64 = 13; /* Permission denied */
pub const EFAULT: i64 = 14; /* Bad address */
pub const ENOTBLK: i64 = 15; /* Block device required */
pub const EBUSY: i64 = 16; /* Device or resource busy */
pub const EEXIST: i64 = 17; /* File exists */
pub const EXDEV: i64 = 18; /* Cross-device link */
pub const ENODEV: i64 = 19; /* No such device */
pub const ENOTDIR: i64 = 20; /* Not a directory */
pub const EISDIR: i64 = 21; /* Is a directory */
pub const EINVAL: i64 = 22; /* Invalid argument */
pub const ENFILE: i64 = 23; /* File table overflow */
pub const EMFILE: i64 = 24; /* Too many open files */
pub const ENOTTY: i64 = 25; /* Not a typewriter */
pub const ETXTBSY: i64 = 26; /* Text file busy */
pub const EFBIG: i64 = 27; /* File too large */
pub const ENOSPC: i64 = 28; /* No space left on device */
pub const ESPIPE: i64 = 29; /* Illegal seek */
pub const EROFS: i64 = 30; /* Read-only file system */
pub const EMLINK: i64 = 31; /* Too many links */
pub const EPIPE: i64 = 32; /* Broken pipe */
pub const EDOM: i64 = 33; /* Math argument out of domain of func */
pub const ERANGE: i64 = 34; /* Math result not representable */

// errno.h
pub const ENOSYS: i64 = 38; /* Invalid system call number */

// fcntl.h
pub const O_RDONLY: u32 = 0o00000000;
pub const O_WRONLY: u32 = 0o00000001;
pub const O_RDWR: u32 = 0o00000002;
pub const O_CREAT: u32 = 0o00000100;
pub const O_ACCMODE: u32 = 0000000003; // AND this to get access mode

// arch/x86/include/uapi/asm/prctl.h
pub const ARCH_SET_GS: u32 = 0x1001;
pub const ARCH_SET_FS: u32 = 0x1002;
pub const ARCH_GET_FS: u32 = 0x1003;
pub const ARCH_GET_GS: u32 = 0x1004;

pub const ARCH_GET_CPUID: u32 = 0x1011;
pub const ARCH_SET_CPUID: u32 = 0x1012;
