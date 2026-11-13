# GRiDLink in Compass-Emu

GRiDLink is the GRiD Server network: the Compass's serial port runs the Intel 8274 in
synchronous SDLC mode at 250 kbit/s (x1 clock from the 80130's baud timer, divisor 20), and
the server, the primary station, polls each Compass in turn (normal response mode, window 1,
modulo 8). A Compass is always a secondary station: it only speaks when polled. So two
Compass-Emus can't link to each other over GRiDLink; the other end of the cable has to play
the GRiD Server.

Compass-Emu 0.9.0 emulates the Compass side: the 8274's SDLC mode, the serial port select
latches and the cable. It does not include a server.

## Using it

    compass --model 1139 --hd disk.img --gridlink tcp:2400          (wait for the server program)
    compass --model 1139 --hd disk.img --gridlink connect:HOST:2400 (connect to it)
    ... --gridlink-p2p                                                (point-to-point cable)

The machine setup page has the same settings under "Serial and sound". The status bar shows
the connection and the number of frames each way.

`--gridlink` and `--serial` use the same port (channel A). With both given, the GRiDLink
cable sets the modem lines. A real PC serial port can't carry SDLC, so `--gridlink` takes
TCP only.

Keep the emulator at real speed while linked. The GRiDLink driver's timeouts (about 180 ms
per byte while it waits for the server's answer, 30 s between polls) count emulated time, so
turbo or the fast core shorten them for the program at the other end.

## Wire format

A TCP stream of messages, the same in both directions:

| bytes | meaning |
| --- | --- |
| 2 | length N of the rest of the message, little-endian |
| 1 | type |
| N - 1 | payload |

| type | payload |
| --- | --- |
| `46h` 'F' | one SDLC frame with a good CRC: address, control, then the rest. GRiDLink's I and XID frames carry a 16-bit little-endian length and the data after the control byte. No flags, no CRC, no bit stuffing. |
| `48h` 'H' | hello, sent once by each side when the connection opens: ASCII `GRiDLink-TCP 1`. |

Any other type is skipped, so a later version can add messages (line state, for example)
without breaking older programs.

The emulator sends a frame once its closing flag has gone out. A frame that arrives is put on
the line at once (or right after the frame before it), one byte per 32 us; the Compass takes
it only if its receiver is on and hunting and the address matches its station address (WR6)
or is FFh. A frame that arrives while the Compass has its receiver off is lost, as on the real
cable, so a server should wait about 2 ms after the Compass's frame before it answers (a real
server needs that long to turn the line around too).

A minimal Python reader:

```python
import socket, struct
s = socket.create_connection(("127.0.0.1", 2400))
def send(kind, payload):
    s.sendall(struct.pack("<H", len(payload) + 1) + bytes([kind]) + payload)
send(0x48, b"GRiDLink-TCP 1")
send(0x46, bytes([0xC0, 0xBF]))          # address poll: XID with P to a new station (C0h)
buf = b""
while True:
    buf += s.recv(4096)
    while len(buf) >= 2 and len(buf) >= 2 + struct.unpack("<H", buf[:2])[0]:
        n = struct.unpack("<H", buf[:2])[0]
        kind, payload, buf = buf[2], buf[3:2 + n], buf[2 + n:]
        if kind == 0x46:
            print("frame", payload.hex(" "))
```

## The cable

GRiDLink~Device~ looks at the cable when it starts: it raises RTS and reads CTS.

- Multidrop (default): CTS is on all the time. The driver sends XID type 1, turns its line
  driver on (serial select latch ST0, DFE10h) only while it sends, and wants DCD before and
  while it reads the server's answer. In the emulator DCD means "someone is driving the line":
  it is on while a frame comes in, and from the end of each frame the Compass sends until the
  end of the next frame it receives (the server turning the line around). CableTest waits for
  DCD to go off before it answers a poll.
- Point-to-point (`--gridlink-p2p`): CTS follows the Compass's own RTS (looped in the cable).
  The driver sends XID type 2, keeps its driver on and ignores DCD.

The latches it writes: multidrop `00 FF FF 00`, point-to-point `FF 00 FF 00` at
DFE10/12/14/16h on the Compass II board (DFE00-DFE06h on the 1101). They are kept in
`GridBoard::serial_select`.

## What the 8274 model does in SDLC mode

From the GRiDLink client code (GRiDLink~Device~ and CableTest), since no 8274 SDLC timing
measurements exist yet:

- Transmit: bytes go out one per 8 baud clocks. When the transmitter runs dry after
  WR0 = C0h (reset Tx Underrun/EOM latch) with Tx CRC on, the 8274 sends the CRC and the
  closing flag, sets the latch (RR0 bit 6) and the frame is complete. Without the latch
  reset the frame is cut off and no station takes it.
- Receive: the FIFO gets the frame, both CRC bytes and, four byte times later, one more
  byte. End of Frame (RR1 bit 7) shows when the second CRC byte is at the top of the FIFO and
  stays until Error Reset. The driver's ShortRead reads four bytes of a short frame and then
  waits, without a timeout, for a fifth before it reads RR1; its LongRead reads one byte after
  End of Frame only if one is there. The late fifth byte satisfies both, on both CPU cores.
- The FIFO holds five bytes (three plus the shift register and the byte ahead of the CRC
  checker, assumed): the receive interrupt (first character only) reaches the driver's first
  read about 900 clocks after the address byte.
- Interrupts: receive on first character, transmit and external/status as in async mode.
  Special receive condition interrupts are not generated (the driver polls RR1). RR2B carries
  the cause in bits 0-2 in 8086 mode (WR2A bits 3-4 = 10), which is what GRiDLink checks
  (cause 6 = channel A receive).

## Testing

`crates/lib/grid_core/tests/gridlink.rs` (ignored, needs a disk and ROMs) boots a Compass,
runs CableTest~Run~ from the GRiD Server disk "Programs for GRiD Compass" and plays the
server's side: address poll (XID+P to C0h), address assignment, then TEST+P polls whose
520-byte answers it sends back. CableTest's own screen counts the exchanges and errors.

    HD_IMG=disk.img ROMS=roms OUT=out MODEL=1139 [EXACT=1] [P2P=1] \
      cargo test --release -p grid_core --test gridlink -- --ignored --nocapture

The disk needs CableTest~Run~ in Programs and no Modem~Device~ there (the emulator has no
modem board, and CableTest hangs activating it). Results (0.9.0): connected, about 500 test
exchanges in 20 s on the fast core and 130 on the exact one, no errors of any kind, on both
cables.
