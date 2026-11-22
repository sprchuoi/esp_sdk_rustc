/* B0: print through UART0 and halt. No calls, no stack, no window spills.
 * ROM has already configured UART0 at 115200 for its own boot log. */
.section .text._start, "ax"
.global _start
.type _start, @function
.align 4
_start:
  movi a3, 0x6000
  slli a3, a3, 16          /* a3 = UART0 base 0x60000000 */
  movi a4, 13
  s32i a4, a3, 0
  movi a4, 10
  s32i a4, a3, 0
  movi a4, 91
  s32i a4, a3, 0
  movi a4, 101
  s32i a4, a3, 0
  movi a4, 115
  s32i a4, a3, 0
  movi a4, 112
  s32i a4, a3, 0
  movi a4, 45
  s32i a4, a3, 0
  movi a4, 115
  s32i a4, a3, 0
  movi a4, 100
  s32i a4, a3, 0
  movi a4, 107
  s32i a4, a3, 0
  movi a4, 93
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 98
  s32i a4, a3, 0
  movi a4, 111
  s32i a4, a3, 0
  movi a4, 111
  s32i a4, a3, 0
  movi a4, 116
  s32i a4, a3, 0
  movi a4, 108
  s32i a4, a3, 0
  movi a4, 111
  s32i a4, a3, 0
  movi a4, 97
  s32i a4, a3, 0
  movi a4, 100
  s32i a4, a3, 0
  movi a4, 101
  s32i a4, a3, 0
  movi a4, 114
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 66
  s32i a4, a3, 0
  movi a4, 48
  s32i a4, a3, 0
  movi a4, 58
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 97
  s32i a4, a3, 0
  movi a4, 108
  s32i a4, a3, 0
  movi a4, 105
  s32i a4, a3, 0
  movi a4, 118
  s32i a4, a3, 0
  movi a4, 101
  s32i a4, a3, 0
  movi a4, 44
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 114
  s32i a4, a3, 0
  movi a4, 117
  s32i a4, a3, 0
  movi a4, 110
  s32i a4, a3, 0
  movi a4, 110
  s32i a4, a3, 0
  movi a4, 105
  s32i a4, a3, 0
  movi a4, 110
  s32i a4, a3, 0
  movi a4, 103
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 102
  s32i a4, a3, 0
  movi a4, 114
  s32i a4, a3, 0
  movi a4, 111
  s32i a4, a3, 0
  movi a4, 109
  s32i a4, a3, 0
  movi a4, 32
  s32i a4, a3, 0
  movi a4, 73
  s32i a4, a3, 0
  movi a4, 82
  s32i a4, a3, 0
  movi a4, 65
  s32i a4, a3, 0
  movi a4, 77
  s32i a4, a3, 0
  movi a4, 13
  s32i a4, a3, 0
  movi a4, 10
  s32i a4, a3, 0
1:
  j 1b                     /* B0 halts here; the emulator stops on --max-seconds */
.size _start, . - _start
