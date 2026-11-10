// Phase-1 speed probe: run demo-style 8086 loops on MartyPC's 8086 core, no devices.
use marty_core::cpu_common::{builder::CpuBuilder, CpuAddress, CpuType, Cpu, Register16};
use std::time::Instant;

fn main() {
    let mut cpu = CpuBuilder::new().with_cpu_type(CpuType::Intel8086).build().expect("cpu");
    // code at 1000:0000
    let prog: Vec<u8> = vec![
        // outer: copy 4800 words 2000:0000 -> 3000:0000, then an unrolled XOR/ROR/ADD loop, then jmp outer
        0xB8,0x00,0x20, 0x8E,0xD8,        // mov ax,2000h ; mov ds,ax
        0xB8,0x00,0x30, 0x8E,0xC0,        // mov ax,3000h ; mov es,ax
        0x31,0xF6, 0x31,0xFF,             // xor si,si ; xor di,di
        0xB9,0xC0,0x12, 0xFC,             // mov cx,4800 ; cld
        0xF3,0xA5,                        // rep movsw
        0xB9,0x00,0x10,                   // mov cx,4096
        0x31,0xDB,                        // xor bx,bx
        // inner: 
        0x8B,0x07,                        // mov ax,[bx]
        0x35,0x55,0xAA,                   // xor ax,0AA55h
        0xD1,0xC8,                        // ror ax,1
        0x89,0x07,                        // mov [bx],ax
        0x01,0x47,0x02,                   // add [bx+2],ax
        0x43,0x43,                        // inc bx ; inc bx
        0xE2,0xF0,                        // loop inner (-16)
        0xEB,0xD2,                        // jmp outer (to 0)
    ];
    // fix jmp displacement: outer at 0, jmp at end
    let mut prog = prog;
    let n = prog.len();
    prog[n-1] = (0i32 - n as i32) as u8;
    for (i,b) in prog.iter().enumerate() { cpu.bus_mut().write_u8(0x10000 + i, *b, 0).unwrap(); }
    cpu.set_reset_vector(CpuAddress::Segmented(0x1000, 0));
    cpu.reset();
    cpu.set_register16(Register16::SS, 0x4000);
    cpu.set_register16(Register16::SP, 0xFFFE);
    let target: u64 = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(200_000_000);
    let t = Instant::now();
    let mut cycles: u64 = 0;
    let mut instr: u64 = 0;
    while cycles < target {
        let (_, c) = cpu.step(false).expect("step");
        let _ = cpu.step_finish(None);
        cycles += c as u64;
        instr += 1;
        if std::env::var("TRACE").is_ok() && instr > 4810 && instr < 4840 {
            println!("{:>6} ip={:04X} +{} clk", instr, cpu.get_ip(), c);
        }
    }
    let s = t.elapsed().as_secs_f64();
    println!("{} cycles, {} instructions in {:.3} s", cycles, instr, s);
    println!("{:.1} M emulated cycles/s = {:.1}x a 5 MHz 8086", cycles as f64 / s / 1e6, cycles as f64 / s / 5e6);
}
