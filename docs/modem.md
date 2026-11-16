# Internal modem

Every Compass except the 1137 has an internal modem board: an 8749 microcontroller running
GRiD's own firmware, a TMS99532 for 300 baud (Bell 103), the 1200 baud (Bell 212A) modem,
a DTMF chip, the sound DACs, the hook relay and the line circuits. From 0.12.0 Compass-Emu
runs the 8749's firmware (`grid_core/src/modem.rs`), so GRiD-OS's `Modem~Device~`, GRiDTerm,
PhoneLink and any program that talks to the modem see the real thing. What the analog parts
would do (tones, carrier, ring) is replaced by what they put on the MCU's pins.

## ROM

Put the 8749 dump in the ROM folder as `16020-007.bin` (2048 bytes, CRC-32 8B526845).
Import ROMs finds it by checksum. `--rom-modem FILE` (or the ROMs card) names another file.
Without it the modem board has no MCU, as before 0.12.0: the DTMF chip still sounds, but
`Modem~Device~` can't start (it waits for the MCU). `--no-modem` (or "Leave the modem MCU
out" on the setup page) leaves it out even with the file there.

## The phone line

The far ends of the phone line are TCP connections, and they carry the raw data bytes
(after the modems have connected), with nothing added. A server program only has to listen on
a TCP port and speak to the Compass the way a GRiD Server's modem would hand it the bytes.

- **The Compass calls.** The number it dials (touch-tone through the DTMF chip, or pulses on
  the hook relay) is looked up in the phone book when it starts the originate handshake:

      --modem-dial 5551234=127.0.0.1:2400 --modem-dial *=myserver:2400

  `*` matches any number not listed. A number written as four address parts and a port
  separated by `*` (127\*0\*0\*1\*2400) dials that address directly. Punctuation in the number
  is ignored. The connection opening is the far modem answering; then it sends its answer
  tone and comes up at the speed the Compass asked for (300 or 1200).
- **Someone calls the Compass.** With `--modem-listen tcp:2400` (or `tcp:0.0.0.0:2400` for
  other PCs) a connection to that port rings the Compass (2 s on, 4 s off). When the Compass
  goes off-hook and answers, the caller's modem comes up at 1200 baud
  (`--modem-answer-300` for 300). Bytes the caller sends before that wait in the socket.
- **Hanging up.** The Compass going on-hook closes the connection; the far end closing it
  drops the carrier, and the Compass's modem reports "carrier lost" after its usual delay.

The setup page has the same settings under "Serial and sound". Data bytes are paced by the
emulated modems at 300 or 1200 baud. The far modem frames bytes as 8N1: when GRiDTerm uses 7
data bits and even parity, the far end gets the parity in bit 7, as a real terminal set to
8N1 would.

Two emulators can call each other: start one with `--modem-listen tcp:2400` and give the
other `--modem-dial *=127.0.0.1:2400`.

## How it is wired (for developers)

From the firmware disassembly (pins) and GRiD's boot PROM source (`InitModem`, the interrupt
table):

| MCU pin | Use |
|---|---|
| DB0-7 | 8255 port A in mode 2: MOVX A,@R0 takes the host's byte (ACK), MOVX @R0,A gives one (STB) |
| P1.0 | 8255 PC2: the host's byte is a command |
| P1.1 | interrupt to the host, IR2 (GRiD interrupt 4, `intModem`) |
| P1.2 | transmit data |
| P1.5 / P1.6 / P1.7 | 1200 transmitter off / originate / 1200 select |
| P2.0 | 8255 OBF#: a byte from the host waits |
| P2.1 / P2.2 | 1200 receive / transmit clock |
| P2.3 | call progress tone (dial tone) |
| P2.4 / P2.5 | 1200 / 300 carrier detect (low = carrier) |
| P2.6 | ring detect (low) |
| P2.7 | hook relay (1 = on-hook) |
| INT | receive data |

8255 PC1 is the MCU's reset (GRiD-OS writes C0h, then 03h to let it run), port B the DTMF
chip, PC0 the voice path. The MCU's clock is 10.7386 MHz (one machine cycle = 440/63 CPU
clocks). The 8749 is a software UART: at 300 baud it times bits with its timer; at 1200 baud
it sends and samples one bit per clock edge and scrambles in software (taps 14 and 17, plus
the 212A rule that inverts a bit after 64 ones). The line model makes the clocks, scrambles
what the far end sends and descrambles what the Compass sends.

Not modelled: the ring line to IR7 (GRiD interrupt 7, `intRing`; GRiD-OS's modem driver
doesn't use it), the use of P1.3/P1.4 (commands 7 and 8), busy and ringback tones, and the
speaker. Dial tone is a plain 400 Hz square wave.

## Tests

- `grid_core/tests/modem.rs` drives the firmware like `Modem~Device~` does and makes calls in
  both directions at 300 and 1200 baud, sending data both ways:

      MODEM_ROM=16020-007.bin cargo test --release -p grid_core --test modem

- `grid_core/tests/modem_boot.rs` (ignored) boots a hard disk and types keys, with a far end
  in the test that answers and echoes in upper case. With the merged development disk on a
  1139, GRiDTerm 3.1.5 dials 5551234, connects at 1200 and echoes what is typed:

      HD_IMG=disk.img ROMS=roms OUT=out BOOT_S=40 STEP_S=20 \
        KEYS="{ret}|Programs{ret}|GRiDTerm{ret}|Run Terminal{ret}|{code+ret}|5551234{code+ret}|hello{ret}" \
        cargo test --release -p grid_core --test modem_boot -- --ignored --nocapture

- `compass` unit tests check the TCP side (`host::ModemLine`): dialing a listening server,
  data both ways, the server hanging up, and an incoming connection ringing.
