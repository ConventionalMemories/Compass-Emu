# 8087 at microcode level

`crates/lib/i8087/src/micro` is a second 8087 core that runs the chip's own microcode ROM
(1648 words of 16 bits) on a model of its datapath: tmpA/tmpB/tmpC, the adder with its B
register and sum register, the shifter, the rounding bits, the micro-stack. It is built
beside the instruction-level core in `lib.rs` and checked against it. Since 0.12.0 the
emulator runs on it whenever the ROM file is there.

## Running the emulator on the microcode

Supply the ROM (next section) and the emulator uses it: the setup page shows the file it
found, the status bar says "8087 microcode", and the headless runner prints "8087: running
on the microcode ROM" at start. Without the file the instruction-level core runs, as before.

To keep the instruction-level core even with the ROM there (for comparing the two): the
setup page's 8087 box ("Instruction-level 8087 even with the microcode ROM"), `--fpu-table`
on the command line, Machine > 8087 on its microcode ROM while the machine runs (the switch
happens between two instructions), or `FPU_CORE=table` headless.

With the microcode every instruction that has a routine in the ROM runs there. The rest is
hardwired in the chip, and stays with the instruction-level core:

- The control instructions (FINIT, FCLEX, FENI, FDISI, FLDCW, FSTCW, FSTSW, FSTENV,
  FLDENV). The ROM has no routines for them.
- The environment part of FSAVE and FRSTOR. Their loops over the eight registers (ROM
  entries 675 and 695) run from the ROM since 0.13.0.

The time an instruction takes comes from the microcode too (see "Timing" below), except
for the control instructions, FSAVE and FRSTOR, which keep the instruction-level core's
tables, corrected to the real 1101 (within about 1.5 clocks).

If the microcode core meets a micro-instruction the model does not know, that instruction
is done by the instruction-level core instead (`I8087::micro_faults` counts them; none are
known).

## The ROM is not included

Compass-Emu does not ship the microcode. The emulator and the tests read it from a file you
supply:

- the file named on the setup page (ROMs, "8087 microcode") or with `--fpu-rom FILE`,
- the environment variable `I8087_MICROCODE` = path to the file, or
- `8087mc.txt` (or `8087mc.bin`) in the ROM folder (`roms` beside the program or under
  the current directory). Import ROMs copies it there by its name.

The text form is the one the "Opcode Collective" dump uses (`tools/8087mc/bin/8087mc.txt` in
github.com/a-mcego/granite): one line per word, 16 columns for bits A..P, a letter for 1 and a
blank for 0. A 3296-byte binary file (big-endian words) works too. Without the file the tests
pass without running and the emulator keeps the instruction-level core.

## Sources

- Ken Shirriff's 8087 articles on righto.com (2018-2026): the microcode engine and its
  conditions, the adder, the stack, FXCH, FSCALE, addition/subtraction, the tangent, the
  constant ROM.
- The microcode dump and field notes by the "Opcode Collective".
- Where neither says what a micro-instruction does, its meaning was worked out by running
  the routine against the IEEE result. Those places are marked "inferred" in `core.rs`.

## What runs from the ROM today

Register and memory forms (the memory operand is read and written as the bus interface's
stream of 16-bit words):

| Instructions | Checked against the instruction-level core on |
| --- | --- |
| FLD/FST/FSTP ST(i), FXCH, FFREE, FNOP, FINCSTP, FDECSTP | zeros, normals, infinities, NaNs, denormals, empties; masked and unmasked |
| FLD1, FLDL2T, FLDL2E, FLDPI, FLDLG2, FLDLN2, FLDZ, FCHS, FABS | same |
| FXAM, FTST, FCOM, FCOMP, FCOMPP | same |
| FADD, FSUB, FSUBR (ST,ST(i) / ST(i),ST / pop forms) | zeros, normals, infinities, NaNs, empties; four rounding modes at 64, 53 and 24 bits; masked and unmasked |
| FLD/FST/FSTP m32, m64, m80 | same, plus denormals |
| FILD/FIST/FISTP m16, m32, m64 | same, plus denormals |
| FADD/FSUB/FSUBR/FCOM/FCOMP with m32, m64, m16int, m32int | as the register forms |
| FMUL, FDIV, FDIVR (register and memory forms) | as FADD |
| FSQRT | same, plus denormals |
| FRNDINT, FSCALE, FXTRACT, FPREM | zeros, normals, infinities, NaNs, empties; all rounding modes; masked and unmasked |
| FBLD, FBSTP | 0 to 18 digit numbers; zeros, normals, denormals, empties |
| FPTAN | 19 results from a real 8087 (Compass 1101, GRiDBench 2.0): 18 bit-exact in both X and Y, one X one unit off; 20000 random arguments against the fitted model |
| FPATAN | 36 results from the real 1101 (GRiDBench 2.2): 34 bit-exact, 2 one unit off (atan of exactly 1/2: the constant ROM's last bits); 20000 random 0 < y < x against a quad precision arctangent; y > x leaves y as the result, as on the real chip |
| F2XM1 | 34 results from the real 1101: 31 bit-exact, 3 one unit off; 20000 random 0 < x <= 0.5 against the quad precision value (at most 2 units off) |
| FYL2X, FYL2XP1 | 15 and 25 results from the real 1101: FYL2X 15 of 15 bit-exact, FYL2XP1 24 of 25 (one a unit off); 20000 random operands each against quad precision |
| FSQRT | also 12 of 12 real 1101 results bit-exact |

FPTAN is the first routine checked against real hardware instead of the old core. It uses
the CORDIC shift register, the arctangent constants and the SQUARE loop. The constant ROM's
bits are not published, so the 16 arctangents are computed (atan(2^-n) scaled to 67 bits,
rounded to nearest); the one result that is a unit off depends on their last bits.

The multiply, divide, square-root and remainder loops are hardware inside the adder that the microcode
only starts (a control word arms it, an adder word runs the loop counter down). Their inner
steps are not documented, so the model computes what the loop leaves behind (product,
quotient or root with guard, round and sticky bits; remainder and quotient) rather than
each step.

Unnormal operands run to the end of the microcode but have no reference to compare with.

Run the comparison with `I8087_MICROCODE=/path/8087mc.txt cargo test -p i8087 micro`.

## What the microcode corrected in the instruction-level core

- Compares write only C3, C2, C0 (C1 is left alone), raise D for a denormal operand, and
  treat infinity as unordered in projective mode (the 8087's default) unless both operands
  are infinite.
- Any NaN operand is invalid; masked, the result is the NaN with the larger fraction. An
  empty operand gives the indefinite NaN.
- Infinity plus or minus infinity is always invalid in projective mode.
- After an unmasked exception nothing is popped.
- FCHS/FABS of an empty register store the indefinite NaN unchanged.
- Precision control rounds the fraction once to 24 or 53 bits and leaves the exponent range
  alone.
- NaNs load from and store to short and long reals as they are (fraction cut to size, no
  exception, no "quieting").
- Loading or using a denormal short/long/temporary real raises D; the load still happens
  unless the stack also overflows.
- A store that raises an unmasked exception writes nothing and pops nothing; FSTP m80 of an
  empty register stores the indefinite NaN.
- FIST/FISTP of a denormal is invalid.
- FRNDINT ignores precision control. FSCALE moves the exponent without rounding, does
  nothing when either register is zero, and is invalid for an infinite scale.
- FXTRACT of zero gives two zeros; of an infinity, NaN or denormal it is invalid.
- FPREM: exact remainder from a shift-and-subtract loop of at most 63 steps (then C2 = 1),
  quotient bits to C0, C3, C1 in every case.
- FBSTP adds one half and chops (no rounding control, never "inexact"): an exact half goes
  toward zero. FBLD keeps the sign of minus zero.
- FSQRT is computed bit by bit and rounded once (rounding and precision control); a
  denormal or (in projective mode) infinite operand is invalid.

## What a real 8087 answered

8087 Bench (a GRiD-OS test program, 2181 tests in two programs) was run on a real 1101 and
a real 1137. Its reports are in `crates/lib/i8087/testdata/` (`bench-table.txt` holds the
tests, `bench-real.txt` the real machines' lines) and the test `bench_real` runs every
test through the microcode core and compares. Of 1588 real result lines all but 34 come out
the same (`bench-known.txt` lists those). What the real chip said about the questions the
microcode reading had left open:

1. Masked overflow while chopping gives infinity.
2. Masked overflow and underflow in round-down and round-up set only the precision flag,
   not O or U (micro-instruction E402 does nothing in those modes).
3. A divisor that is not normalized (a denormal, an unnormal, a pseudo-zero) makes FDIV
   invalid. FPREM by zero is invalid too.
4. FSCALE adds the exponents in 17 bits. Bit 16 of the sum means underflow, otherwise bit
   15 (or 7FFF) means overflow.
5. The chip trusts the tag word and not the register's bits, and a result gets the tag of
   the temporary register it came from.
6. FBSTP of a NaN, infinity or too large a number stores the indefinite with FFFF in the
   sign word.
7. FLD m80 of a denormal already sets the D flag.
8. The control word has 13 bits (FFFF reads back as 1FFF). The IR bit of the status word is
   derived from the flags and masks, it is not stored. The B bit is set as long as an
   unmasked exception is pending.
9. FINIT and FSAVE keep the condition bits C3 to C0.
10. FSTENV's instruction pointer points at the ESC byte (after a segment prefix), and
    FSTSW and FSAVE do not change the pointers.
11. Reserved opcodes alias their neighbours: DA C0-FF as D8, DB C0-C7 as D9, DF C8-CF as
    D9 C8 (FXCH), DF D0-DF as DD D0-DF, D9 D8-DF pops only.
12. The 8087's interrupt arrives on the 80130's IR6, not on NMI.

Still open:

- The constant ROM's own bits. Two arctangent and five logarithm constants were moved by
  one or two units in the last place to match the real results (`fit_constants` shows the
  search; `FB_FIT=frac` tries ln(2)/3, log2(e), 3 log2(e) and sqrt(2) too and finds nothing
  better). Thirty transcendental lines still differ. About twenty are FYL2X, FYL2XP1 and
  F2XM1 results where the value before rounding is a fraction of a unit off: the same
  result with the other rounding direction in C1, or one unit apart. F2XM1 comes out one
  unit low five times, the logarithms a fraction high. The rest are arguments far outside
  the documented range (F2XM1 above 0.5 or of a NaN, FYL2XP1 of 1 and of -0.5, FPATAN of
  0/0), where the result depends on register widths the model does not have right.
- Control word 6E44 (F2XM1, before the denominator's subtraction) is a no-op in the model.
  It is not a carry or a rounding of that subtraction: a unit either way there is the same
  as a unit on log2(e), and neither matches more real results.
- The registers after power-on hold what a real 1101's held (all eight
  FCA9AFEAA3BE028FA027), so FXAM of an empty register answers 4700 as on both real
  machines. A real 1137's eight registers differ from each other; the model uses the
  1101's pattern on every model.

## Timing

The busy time of an instruction is a fixed part for its form plus a cost for every
micro-instruction that ran, by kind (`micro::KIND_CLOCKS`, `micro::clocks`):

| Kind | Clocks |
| --- | --- |
| register transfer | 0.94 |
| immediate | 0.57 |
| shift by a fixed count | 1.07 |
| shift by a variable count | 0.48 |
| add or subtract | 2.05 |
| control | 1.06 |
| jump taken | 2.10 |
| jump not taken | 1.90 |
| call | 2.08 |
| return | 2.08 |
| other | 0.95 |
| one step of the multiply loop | 1.98 |
| one step of the divide loop | 1.99 |
| one step of the square root loop | 1.94 |
| word read on the bus | 4.46 |
| word written on the bus | 1.13 |
| one step of FPREM's loop | 1.97 |
| one step of the transcendentals' quotient loop | 1.98 |
| bit a variable shift moves its operand by | 0.02 |
| the same for a shift by the CORDIC loop counter | 0.07 |

The costs and the fixed parts are a least-squares fit to the clocks 8087 Bench measured on
a real 1101 for about 2070 tests, reweighted so that a few odd tests do not steer it. They
are costs that add up right, not the chip's own cycle counts (the fixed part of a memory
form is negative because its bus words are counted on top). Over the timed tests the
emulator's mean error is 1.45 % against the real 1101 (about 1.5 clocks); with the
instruction-level core's tables it was 40 %. What is left is mostly how the CPU's WAIT
lines up with the end of the busy time.

The control instructions are hardwired in the chip, and FSAVE and FRSTOR mostly so, so
their times stay table times, corrected by a few clocks each to the real 1101 (`micro::control_fit`); they
are within about 1.5 clocks now. The largest errors left are FCOMPP, the far out-of-range
transcendental arguments and FBLD/FBSTP of some values.

## WAIT while BUSY is held

With an unmasked exception pending and interrupts enabled in the control word, the 8087
keeps BUSY active until the exception is cleared. A WAIT then never ends: the 8086 stays
in the instruction, takes interrupts there and returns to it. Both CPU cores do the same
now (after 4096 polls of TEST the WAIT starts over at its own address), so a program that
hangs on the real machine hangs in the emulator: 8087 Bench 0.0.3's risky part stops at
y005, as it did on the real 1101.

## FSAVE and FRSTOR

The environment part of both is hardwired in the chip; the eight registers go through two
microcode loops (FSAVE at 695, FRSTOR at 675), and the microcode core runs those from the
ROM, with the bus words handed to it and taken from it one by one. The results are the
same as the instruction-level core's. The time is still one measured figure per
instruction: a single real pair cannot say how it splits over the loop's steps.

## The constant ROM and 8087 Bench 3

The transcendentals' constants (16 arctangents, 16 logarithms, 4 fractions) are computed,
not read from a chip, so their last bits are guesses; the thirty lines above hang on them.
8087 Bench 0.0.5 has a third program whose 1584 tests each lean on one constant: FYL2X
with 17 values of x that each use a single logarithm constant, F2XM1 around
log2(1 + 2^-n), FPATAN of 2^-n and FPTAN around atan 2^-n, each a few units either side.
`probe_uses` (FB_USES) lists which constants a test touches and `probe_sensitivity`
(FB_SENS) how many of the tests change when a constant moves by a unit: every logarithm
constant from 2 to 15 moves 3 to 50 results, the arctangents fewer (some only show a move
of two units). Once a real machine has run it, `fit_constants` can pin the bits down.
