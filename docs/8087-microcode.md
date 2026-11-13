# 8087 at microcode level (work in progress)

`crates/lib/i8087/src/micro` is a second 8087 core that runs the chip's own microcode ROM
(1648 words of 16 bits) on a model of its datapath: tmpA/tmpB/tmpC, the adder with its B
register and sum register, the shifter, the rounding bits, the micro-stack. It is built
beside the instruction-level core in `lib.rs` and checked against it; the emulator does not
use it yet.

## The ROM is not included

Compass-Emu does not ship the microcode. The tests (and later the emulator) read it from a
file you supply:

- the environment variable `I8087_MICROCODE` = path to the file, or
- `roms/8087mc.txt` (or `roms/8087mc.bin`) under the current directory.

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
| FPATAN | 20000 random 0 < y < x against a quad precision arctangent: at most 1 unit off when y is within 2^12 of x, up to 5 units (always low) for smaller ratios, exact where it falls back to a plain division (ratio under 2^-63); y > x leaves y as the result, as measured on the real 1101 |

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

## Open questions for a real 8087

The microcode reading and the old core differ here, and only a real chip can say:

1. Masked overflow while chopping (RC = 11): infinity (microcode reading) or the largest
   number?
2. Masked overflow in round-down/round-up giving the largest number: are all 64 fraction
   bits set even at 24/53-bit precision, and is O flagged?
3. Underflow at 24/53-bit precision: is the denormal rounded again from the top of the
   fraction?
4. Adding a number 64 or more binary places smaller: which rounding bits does the chip keep
   (assumed: the true guard/round/sticky)?
5. Denormal operands in FADD/FSUB: the microcode turns them into unnormals with exponent 1
   and the result stays an unnormal with exponent 1.
6. FMUL/FDIV results that underflow: the microcode rounds to 64 bits first and then again
   as a denormal (double rounding, one unit off now and then).
7. A denormal short or long real as a memory operand of FADD/FSUB/FMUL/FDIV stays an
   unnormal, and so does the result.
8. FSCALE with 0 < |ST(1)| < 1 or |ST(1)| >= 2^15 (Intel: undefined): the microcode shifts
   by a truncated count.
9. FPREM by zero: the microcode has no test for it and runs the loop with a zero divisor.
10. FBSTP of a NaN, infinity or too large a number: which bytes are under the FFFF?
11. The constant ROM's own bits (arctangents and logarithms, 67 bits each): one FPTAN result
    in 19 is a unit off with computed constants.
12. FPATAN on small ratios: the microcode's chopped fixed-point divisions lose 2 to 5 units
    of the last place when y is 2^13 or more below x. No real results exist to confirm
    this (real-hw-tests item F1 would).

## Not there yet

F2XM1, FYL2X, FYL2XP1, the environment
instructions (FLDCW, FSTENV, FSAVE...), clocks per micro-instruction, and the switch that
lets the emulator run on this core.
