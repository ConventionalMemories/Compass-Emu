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

Register forms only (no memory operands yet):

| Instructions | Checked against the instruction-level core on |
| --- | --- |
| FLD/FST/FSTP ST(i), FXCH, FFREE, FNOP, FINCSTP, FDECSTP | zeros, normals, infinities, NaNs, denormals, empties; masked and unmasked |
| FLD1, FLDL2T, FLDL2E, FLDPI, FLDLG2, FLDLN2, FLDZ, FCHS, FABS | same |
| FXAM, FTST, FCOM, FCOMP, FCOMPP | same |
| FADD, FSUB, FSUBR (ST,ST(i) / ST(i),ST / pop forms) | zeros, normals, infinities, NaNs, empties; four rounding modes at 64, 53 and 24 bits; masked and unmasked |

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
6. FPTAN: does the CORDIC path start at exponent -16 (Shirriff's reading of the constant)
   or -15 (the earlier fit to the 1101 dumps)?

## Not there yet

Memory operands (the bus interface word stream), FMUL/FDIV/FSQRT (hardware loops), FPREM,
FRNDINT, FSCALE, FXTRACT, the transcendentals, BCD, clocks per micro-instruction.
