/* B0: print the product banner through UART0 and halt.
 *
 * No calls, no stack, no window spills. The banner lives in .rodata and is
 * walked byte by byte; the only literal is a pointer to it, placed in
 * .literal before the code (l32r can only reach backwards).
 *
 * Change the message in .Lbanner — it is the only thing to edit.
 */

.section .literal, "a"
.align 4
.Lbanner_ptr:
  .word .Lbanner

.section .rodata, "a"
.Lbanner:
  .asciz "\r\nSprchuoi ESP32 Boot\r\n[esp-sdk] B0 stage-2, running from IRAM\r\n"

.section .text._start, "ax"
.global _start
.type _start, @function
.align 4
_start:
  movi a3, 0x6000
  slli a3, a3, 16            /* a3 = UART0 base 0x60000000 */
  l32r a5, .Lbanner_ptr      /* a5 = &banner */
1:
  l8ui a4, a5, 0
  beqz a4, 2f
  s32i a4, a3, 0             /* push the byte into the UART FIFO */
  addi a5, a5, 1
  j 1b
2:
  j 2b                       /* B0 halts; the emulator stops on --max-seconds */
.size _start, . - _start
