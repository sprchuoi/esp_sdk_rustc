/* B0 bootloader: code and literals in IRAM, no data section needed.
 * The ROM loads segments from the image header straight to these addresses. */
ENTRY(_start)

SECTIONS
{
  . = 0x403C0000;
  .text : {
    /* l32r literals must precede the code that loads them. */
    *(.literal .literal.*)
    KEEP(*(.text._start))
    *(.text .text.*)
    *(.rodata .rodata.*)
  }

  /DISCARD/ : {
    *(.comment)
    *(.note.*)
    *(.xtensa.*)
  }
}
