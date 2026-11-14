# MartyPC

Compass-Emu is built on [MartyPC](https://github.com/dbalsom/martypc), a cycle-accurate IBM PC/XT
emulator by Daniel Balsom, released under the MIT license (see [LICENSE](LICENSE)).

What Compass-Emu keeps from MartyPC:

- `crates/lib/marty_core`: the cycle-accurate 8086/8088 CPU core with its prefetch queue and
  bus timing, the bus framework (extended here with an external board interface for the
  Compass memory map, I/O ports, wait states and interrupts) and the 8259 interrupt controller.
  Its PC devices (video cards, floppy and hard disk controllers, sound cards and so on) are not
  used by the Compass.
- `crates/lib/marty_common`: shared helper types.

MartyPC's PC frontends, display back ends, GUI panels and bundled PC software were removed
from this repository. For MartyPC itself, its user guide, releases and changelog, see
[github.com/dbalsom/martypc](https://github.com/dbalsom/martypc) and [martypc.net](https://martypc.net).
