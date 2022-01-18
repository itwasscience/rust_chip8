use log::{debug, error};
use pixels::{Error, Pixels, SurfaceTexture};
use rand::Rng;
use std::thread::current;
use winit::dpi::LogicalSize;
use winit::event::{Event, VirtualKeyCode};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use winit_input_helper::WinitInputHelper;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;

#[derive(Debug)]
enum EmulationStatus {
    Running,
    WaitingForKey,
}

#[derive(Debug)]
struct Chip8 {
    status: EmulationStatus,
    pc: usize,                   // Program Counter
    sp: usize,                   // Stack Pointer
    memory: [u8; 4000],          // 4000 Bytes - Standard Chip8
    registers: [u8; 16],         // 0xF is Flag Register
    address_reg: u16,            // Technically 12-bits
    stack: [usize; 12],          // 12 levels of nesting
    delay_timer: u8,             // Ticks down at 60 hz
    sound_timer: u8,             // Ticks down at 60 hz
    input: u8,                   // Only one button at any time
    video_buffer: [u8; 64 * 32], // 1 Byte per Pixel
    redraw: bool,                // Flag for redraw request on video_buffer change
}
impl Chip8 {
    fn tick(&mut self) {
        self.exec_opcode();
    }

    fn exec_opcode(&mut self) {
        // Break out the opcodes into four nibbles for pattern matching
        let high_byte: u8 = self.memory[self.pc];
        let low_byte: u8 = self.memory[self.pc + 1];

        let opcode = ((high_byte as u16) << 8) | low_byte as u16;
        let nibbles = (
            (opcode & 0xF000) >> 12 as u8,
            (opcode & 0x0F00) >> 8 as u8,
            (opcode & 0x00F0) >> 4 as u8,
            (opcode & 0x000F) as u8,
        );
        let nnn: usize = (opcode & 0x0FFF).into();
        let nn: usize = (opcode & 0x00FF).into();
        let x: usize = nibbles.1.into();
        let y: usize = nibbles.2.into();
        let n: usize = nibbles.3.into();

        self.pc = match nibbles {
            (0x00, 0x00, 0x0E, 0x00) => self.opcode_00e0(),
            (0x00, 0x00, 0x0E, 0x0E) => self.opcode_00ee(),
            (0x01, _, _, _) => self.opcode_1nnn(nnn),
            (0x02, _, _, _) => self.opcode_2nnn(nnn),
            (0x03, _, _, _) => self.opcode_3xnn(x, nn),
            (0x04, _, _, _) => self.opcode_4xnn(x, nn),
            (0x05, _, _, _) => self.opcode_5xnn(x, y),
            (0x06, _, _, _) => self.opcode_6xnn(x, nn),
            (0x07, _, _, _) => self.opcode_7xnn(x, nn),
            (0x08, _, _, 0x00) => self.opcode_8xy0(x, y),
            (0x08, _, _, 0x01) => self.opcode_8xy1(x, y),
            (0x08, _, _, 0x02) => self.opcode_8xy2(x, y),
            (0x08, _, _, 0x03) => self.opcode_8xy3(x, y),
            (0x08, _, _, 0x04) => self.opcode_8xy4(x, y),
            (0x08, _, _, 0x05) => self.opcode_8xy5(x, y),
            (0x08, _, _, 0x06) => self.opcode_8xy6(x),
            (0x08, _, _, 0x07) => self.opcode_8xy7(x, y),
            (0x08, _, _, 0x0E) => self.opcode_8xye(x),
            (0x09, _, _, _) => self.opcode_9xy0(x, y),
            (0x0A, _, _, _) => self.opcode_annn(nnn),
            (0x0B, _, _, _) => self.opcode_bnnn(nnn),
            (0x0C, _, _, _) => self.opcode_cxnn(x, nn),
            (0x0D, _, _, _) => self.opcode_dxyn(x, y, n),
            (0x0E, _, 0x09, 0x0E) => self.opcode_ex9e(x),
            (0x0E, _, 0x0A, 0x01) => self.opcode_exa1(x),
            (0x0F, _, 0x00, 0x07) => self.opcode_fx07(x),
            (0x0F, _, 0x00, 0x0A) => self.opcode_fx0a(x),
            (0x0F, _, 0x01, 0x05) => self.opcode_fx15(x),
            (0x0F, _, 0x01, 0x08) => self.opcode_fx18(x),
            (0x0F, _, 0x01, 0x0E) => self.opcode_fx1e(x),
            (0x0F, _, 0x02, 0x09) => self.opcode_fx29(x),
            (0x0F, _, 0x03, 0x03) => self.opcode_fx33(x),
            (0x0F, _, 0x05, 0x05) => self.opcode_fx55(x),
            (0x0F, _, 0x06, 0x05) => self.opcode_fx65(x),
            _ => self.pc, // Do Nothing
        }
    }
    // Clear Screen
    fn opcode_00e0(&mut self) -> usize {
        debug!("00E0, Clear Screen");
        self.video_buffer = [0; 32 * 64];
        self.redraw = true;
        self.pc + 2
    }
    // Return
    fn opcode_00ee(&mut self) -> usize {
        debug!("00EE, Return");
        let pc = self.stack[self.sp as usize] as u16;
        self.sp -= 1;
        pc.into()
    }
    // Jump to nnn
    fn opcode_1nnn(&mut self, nnn: usize) -> usize {
        debug!("1NNN, Jmp to {:#04x}", nnn);
        nnn.into()
    }
    // Call sub-routine at nnn
    fn opcode_2nnn(&mut self, nnn: usize) -> usize {
        log::debug!("2NNN, Call {:#04x}", nnn);
        self.sp += 1;
        self.stack[self.sp] = self.pc + 2;
        nnn.into()
    }
    // If (Vx == NN)
    fn opcode_3xnn(&mut self, x: usize, nn: usize) -> usize {
        log::debug!("3xNN, Vx == NN");
        match self.registers[x] == nn as u8 {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // If (Vx != NN)
    fn opcode_4xnn(&mut self, x: usize, nn: usize) -> usize {
        log::debug!("4xNN, Vx != NN");
        match self.registers[x] != nn as u8 {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // If (Vx == Vy)
    fn opcode_5xnn(&mut self, x: usize, y: usize) -> usize {
        log::debug!("5xNN, Vy == Vx");
        match self.registers[x] == self.registers[y] {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // Set Vx to NN
    fn opcode_6xnn(&mut self, x: usize, nn: usize) -> usize {
        self.registers[x] = nn as u8;
        self.pc + 2
    }
    // Add Vx to NN
    fn opcode_7xnn(&mut self, x: usize, nn: usize) -> usize {
        let (sum, _overflow) = self.registers[x].overflowing_add(nn as u8);
        self.registers[x] = sum;
        self.pc + 2
    }

    // Vx = Vy
    fn opcode_8xy0(&mut self, x: usize, y: usize) -> usize {
        self.registers[x] = self.registers[y];
        self.pc + 2
    }
    // Vx | Vy
    fn opcode_8xy1(&mut self, x: usize, y: usize) -> usize {
        self.registers[x] |= self.registers[y];
        self.pc + 2
    }
    // Vx & Vy
    fn opcode_8xy2(&mut self, x: usize, y: usize) -> usize {
        self.registers[x] &= self.registers[y];
        self.pc + 2
    }
    // Vx ^ Vy
    fn opcode_8xy3(&mut self, x: usize, y: usize) -> usize {
        self.registers[x] ^= self.registers[y];
        self.pc + 2
    }
    // Vx += Vy with Carry
    fn opcode_8xy4(&mut self, x: usize, y: usize) -> usize {
        let (sum, overflow) = self.registers[x].overflowing_add(self.registers[y]);
        if overflow {
            self.registers[0xF] = 1;
        }
        self.registers[x] = sum;
        self.pc + 2
    }
    // Vx -= Vy with Borrow Flag
    fn opcode_8xy5(&mut self, x: usize, y: usize) -> usize {
        let (difference, overflow) = self.registers[x].overflowing_sub(self.registers[y]);
        match overflow {
            true => self.registers[0xF] = 0,
            false => self.registers[0xF] = 1,
        }
        self.registers[x] = difference;
        self.pc + 2
    }
    // Vx >>= 1, save LSB in Flag
    fn opcode_8xy6(&mut self, x: usize) -> usize {
        self.registers[0xF] = self.registers[x] & 0x01;
        self.registers[x] >>= 1;
        self.pc + 2
    }
    // Vx = Vy - Vx with Borrow Flag
    fn opcode_8xy7(&mut self, x: usize, y: usize) -> usize {
        let (difference, overflow) = self.registers[x].overflowing_sub(self.registers[y]);
        if overflow {
            self.registers[0xF] = 1;
        }
        self.registers[x] = difference;
        self.pc + 2
    }

    // Vx <<= 1, save MSB in Flag
    fn opcode_8xye(&mut self, x: usize) -> usize {
        self.registers[0xF] = self.registers[x] & 0x80;
        self.registers[x] <<= 1;
        self.pc + 2
    }
    // If (Vx != Vy)
    fn opcode_9xy0(&mut self, x: usize, y: usize) -> usize {
        match self.registers[x] != self.registers[y] {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // I = nnn
    fn opcode_annn(&mut self, nnn: usize) -> usize {
        self.address_reg = nnn as u16;
        self.pc + 2
    }
    // PC = V0 + nnn
    fn opcode_bnnn(&mut self, nnn: usize) -> usize {
        self.registers[0] as usize + nnn
    }
    // Vx = rand & nn
    fn opcode_cxnn(&mut self, x: usize, nn: usize) -> usize {
        let mut rng = rand::thread_rng();
        let num: u8 = rng.gen();
        self.registers[x] = (nn & num as usize) as u8;
        self.pc + 2
    }
    // Draw(Vx, Vy, N), N = height
    fn opcode_dxyn(&mut self, x: usize, y: usize, n: usize) -> usize {
        self.registers[0xF] = 0; // Reset collision detection
        for row in 0..n {
            let x_coord: usize = self.registers[x] as usize;
            let y_coord: usize = self.registers[y] as usize;
            let video_addr: usize = (y_coord + row) * 64 + (x_coord);

            for bit in 0..8 {
                let color = self.memory[self.address_reg as usize + row] >> (7 - bit) & 0x1;
                // Any pixel collision anywhere may flip this to true
                self.registers[0xF] |= self.video_buffer[(video_addr + bit) % 2048] & color;
                self.video_buffer[(video_addr + bit) % 2048] ^= color;
            }
        }
        self.redraw = true;
        self.pc + 2
    }
    // If key == Vx
    fn opcode_ex9e(&mut self, x: usize) -> usize {
        match self.registers[x] == self.input {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // If key != Vx
    fn opcode_exa1(&mut self, x: usize) -> usize {
        match self.registers[x] != self.input {
            true => self.pc + 4,
            false => self.pc + 2,
        }
    }
    // Vx = get_delay()
    fn opcode_fx07(&mut self, x: usize) -> usize {
        self.registers[x] = self.delay_timer;
        self.pc + 2
    }
    // Vx = get_key()
    fn opcode_fx0a(&mut self, x: usize) -> usize {
        self.registers[x] = self.input;
        self.pc + 2
    }
    // Set Delay to Vx
    fn opcode_fx15(&mut self, x: usize) -> usize {
        self.delay_timer = x as u8;
        self.pc + 2
    }
    // Set Sound to Vx
    fn opcode_fx18(&mut self, x: usize) -> usize {
        self.sound_timer = x as u8;
        self.pc + 2
    }
    // Add Vx to I
    fn opcode_fx1e(&mut self, x: usize) -> usize {
        self.address_reg += x as u16;
        self.pc + 2
    }
    // Set I to Sprite Address Location (Font)
    fn opcode_fx29(&mut self, x: usize) -> usize {
        self.address_reg = self.memory[x * 5] as u16;
        self.pc + 2
    }
    // Store BCD of Vx into I (hundreds), I+1 (tens), I+2 (ones)
    fn opcode_fx33(&mut self, x: usize) -> usize {
        self.memory[self.address_reg as usize] = self.registers[x] / 100;
        self.memory[self.address_reg as usize + 1] = (self.registers[x] % 100) / 10;
        self.memory[self.address_reg as usize + 2] = self.registers[x] % 10;
        self.pc + 2
    }
    // Dump registers from V0 to Vx into Memory, starting at I
    fn opcode_fx55(&mut self, x: usize) -> usize {
        for i in 0x0..x + 1 {
            self.memory[self.address_reg as usize + i] = self.registers[i];
        }
        self.pc + 2
    }
    // Load registers from I
    fn opcode_fx65(&mut self, x: usize) -> usize {
        for i in 0x0..x + 1 {
            self.registers[i] = self.memory[self.address_reg as usize + i];
        }
        self.pc + 2
    }

    fn draw(&mut self, frame: &mut [u8]) {
        if self.redraw {
            let mut rng = rand::thread_rng();
            // Green for normal, amber on beeps
            let color = match self.sound_timer {
                0 => [0xFA, 0xFA, 0x10, 0xFF],
                _ => [0x10, 0xFA, 0x10, 0xFF]
            };
            // Flip the buffer into the RGBA space
            for (i, pixel) in frame.chunks_exact_mut(4).enumerate() {
                let mut rgba = [0; 4];
                match self.video_buffer[i] {
                    1 => rgba = color,
                    _ => rgba = [0x10, 0x10, 0x10, 0xFF],
                }
                pixel.copy_from_slice(&rgba);
            }
        }
        self.redraw = false;
    }
    fn load_font(&mut self) {
        let font = [
            0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
            0x20, 0x60, 0x20, 0x20, 0x70, // 1
            0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
            0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
            0x90, 0x90, 0xF0, 0x10, 0x10, // 4
            0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
            0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
            0xF0, 0x10, 0x20, 0x40, 0x40, // 7
            0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
            0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
            0xF0, 0x90, 0xF0, 0x90, 0x90, // A
            0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
            0xF0, 0x80, 0x80, 0x80, 0xF0, // C
            0xE0, 0x90, 0x90, 0x90, 0xE0, // D
            0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
            0xF0, 0x80, 0xF0, 0x80, 0x80, // F
        ];
        for i in 0..80 {
            self.memory[i] = font[i];
        }
    }
    fn load_rom(&mut self) {
        match std::fs::read("./roms/brix.ch8") {
            Ok(bytes) => {
                for (i, byte) in bytes.iter().enumerate() {
                    self.memory[0x200 + i] = *byte;
                }
            }
            Err(e) => {
                panic!("{}", e);
            }
        }
    }
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let event_loop = EventLoop::new();
    let mut input = WinitInputHelper::new();
    let window = {
        let size = LogicalSize::new(WIDTH as f64, HEIGHT as f64);
        WindowBuilder::new()
            .with_title("Chip8")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .build(&event_loop)
            .unwrap()
    };

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        Pixels::new(WIDTH, HEIGHT, surface_texture)?
    };

    let mut cpu = Chip8 {
        status: EmulationStatus::Running,
        pc: 0x200,
        sp: 0,
        memory: [0; 4000],
        registers: [0; 16],
        address_reg: 0,
        stack: [0; 12],
        delay_timer: 0,
        sound_timer: 0,
        input: 0,
        video_buffer: [0; 64 * 32],
        redraw: false,
    };
    cpu.load_font();
    cpu.load_rom();

    let mut current_delay_timer = std::time::Instant::now();
    let mut current_sound_timer = std::time::Instant::now();

    event_loop.run(move |event, _, control_flow| {
        // Draw the current frame
        if let Event::RedrawRequested(_) = event {
            cpu.draw(pixels.get_frame());

            if pixels
                .render()
                .map_err(|e| error!("pixels.render() failed: {}", e))
                .is_err()
            {
                *control_flow = ControlFlow::Exit;
                return;
            }
        }
        /*    Key Mappings
         * Chip8       QWERTY
         * 1 2 3 C     1 2 3 4
         * 4 5 6 D >>> Q W E R
         * 7 8 9 E >>> A S D F
         * A 0 B F     Z X C V
         *
         */
        if input.update(&event) {
            // Close events
            if input.key_pressed(VirtualKeyCode::Escape) || input.quit() {
                *control_flow = ControlFlow::Exit;
                return;
            }
            if input.key_held(VirtualKeyCode::Key1) {
                cpu.input = 0x01;
            } else if input.key_held(VirtualKeyCode::Key2) {
                cpu.input = 0x02;
            } else if input.key_held(VirtualKeyCode::Key2) {
                cpu.input = 0x03;
            } else if input.key_held(VirtualKeyCode::Key3) {
                cpu.input = 0x03;
            } else if input.key_held(VirtualKeyCode::Key4) {
                cpu.input = 0x0C;
            } else if input.key_held(VirtualKeyCode::Q) {
                cpu.input = 0x04;
            } else if input.key_held(VirtualKeyCode::W) {
                cpu.input = 0x05;
            } else if input.key_held(VirtualKeyCode::E) {
                cpu.input = 0x06;
            } else if input.key_held(VirtualKeyCode::R) {
                cpu.input = 0x0D;
            } else if input.key_held(VirtualKeyCode::A) {
                cpu.input = 0x07;
            } else if input.key_held(VirtualKeyCode::S) {
                cpu.input = 0x08;
            } else if input.key_held(VirtualKeyCode::D) {
                cpu.input = 0x09;
            } else if input.key_held(VirtualKeyCode::F) {
                cpu.input = 0x0E;
            } else if input.key_held(VirtualKeyCode::Z) {
                cpu.input = 0x0A;
            } else if input.key_held(VirtualKeyCode::X) {
                cpu.input = 0x00;
            } else if input.key_held(VirtualKeyCode::C) {
                cpu.input = 0x0B;
            } else if input.key_held(VirtualKeyCode::V) {
                cpu.input = 0x0F;
            } else {
                cpu.input = 0x00;
            }

            // Resize the window
            if let Some(size) = input.window_resized() {
                pixels.resize_surface(size.width, size.height);
            }
            // Update internal state and request a redraw
            cpu.tick();
            window.request_redraw();
            // 60 Hz Delay Clock
            let delay_check = current_delay_timer.elapsed();
            if delay_check.as_secs() > 1 {
                let (value, overflow) = cpu.delay_timer.overflowing_sub(1);
                match overflow {
                    true => cpu.delay_timer = 0,
                    false => cpu.delay_timer -= 1,
                }
                current_delay_timer = std::time::Instant::now();
            }
            // 60 Hz Sound Clock
            let sound_check = current_sound_timer.elapsed();
            if sound_check.as_secs() > 1 {
                let (value, overflow) = cpu.sound_timer.overflowing_sub(1);
                match overflow {
                    true => cpu.sound_timer = 0,
                    false => cpu.sound_timer -= 1,
                }
                current_sound_timer = std::time::Instant::now();
            }
        }
    });
}
