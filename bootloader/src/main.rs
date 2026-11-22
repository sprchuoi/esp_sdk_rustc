//! B0 of the custom bootloader: an image at flash 0x0 that the mask ROM loads
//! into IRAM, which prints a banner and halts.
//!
//! No Rust code runs in B0 — the whole program is assembly, so there is no
//! stack use, no windowed-ABI spill and no runtime to bring up yet. The panic
//! handler exists only to satisfy the language; it is never called.

#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

core::arch::global_asm!(include_str!("boot.S"));

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
