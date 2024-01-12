use crate::internal::ppu::{PPU, Display};
use crate::internal::timer::Timer;
use crate::internal::apu::APU;
use crate::{u32_to_little_endian, console_log, log};

const MBC_TYPE: usize = 0x0147;
const RAM_SIZE: usize = 0x0149;

#[derive(PartialEq)]
enum BankingMode {
    SIMPLE, ADVANCED
}

#[derive(PartialEq, Debug)]
enum MemoryBank {
    MBCNONE, MBC1, MBC1M, MBC3
}

pub struct Memory {
    // testing
    pub flat_ram: bool,

    // used for save files
    pub bess_buffer_offsets: Vec<u8>, 

    rom_chip: Vec<u8>,
    wram: [u8; 0x2000],
    hram: [u8; 0x7F],
    sram: Vec<u8>, // resize to fit all banks of cartridge (if any)

    boot_rom: [u8; 0x100],
    mbc_ram_enabled: bool,
    boot_rom_mounted: bool,

    memory_bank: MemoryBank,
    banking_mode: BankingMode,
    rom_bank_number: u8,
    ram_rom_bank_number: u8,

    pub IE: u8,
    pub IF: u8,

    pub keypress: i8,
    joyp: u8,

    ppu: PPU,
    apu: APU,
    timer: Timer
}

impl Memory {
    const NINTENDO_LOGO: [u8; 48] = [0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
                                      0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
                                      0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E];

    pub fn mount_bootrom(&mut self, bytes: Vec<u8>) {
        for i in 0..bytes.len() {
            self.boot_rom[i] = bytes[i];
        }
        self.boot_rom_mounted = true;
    }

    pub fn load_cartridge(&mut self, bytes: Vec<u8>) {
        self.rom_chip = bytes;

        match self.rom_chip[RAM_SIZE] {
            0x00 => (), // No RAM
            0x01 => (), // Unused
            0x02 => self.sram.resize(0x2000, 0x00), // 1 bank
            0x03 => self.sram.resize(0x2000 * 4, 0x00), // 4 banks of 8 KiB each
            0x04 => self.sram.resize(0x2000 * 16, 0x00), // 16 banks of 8 KiB each
            0x05 => self.sram.resize(0x2000 * 8, 0x00), // 8 banks of 8 KiB each
            _ => ()
        }

        match self.rom_chip[MBC_TYPE] {
            0x00 => {
                self.memory_bank = MemoryBank::MBCNONE;
                self.rom_chip.resize(0x10000, 0x00);
            },
            0x01..=0x03 => {
                self.memory_bank = MemoryBank::MBC1;

                // checks if MBC1M instead
                let mut logo_ptr = (Memory::NINTENDO_LOGO.len() - 1) as i8;
                for i in (0x4000..0x8000).rev() {
                    if logo_ptr < 0 {
                        self.memory_bank = MemoryBank::MBC1M;
                        panic!("not implemented MBC1M yet!");
                    }

                    let bank_ten_ptr = ((0x10 as u32) << 14) | (i & 0x3FFF);
                    if bank_ten_ptr >= self.rom_chip.len() as u32 { break } // out of bounds
                    if self.rom_chip[bank_ten_ptr as usize] == Memory::NINTENDO_LOGO[logo_ptr as usize] {
                        logo_ptr -= 1;
                    } else {
                        logo_ptr = (Memory::NINTENDO_LOGO.len() - 1) as i8;
                    }
                }
            },
            0x0F..=0x13 => self.memory_bank = MemoryBank::MBC3,
            _ => panic!("MBC NOT IMPLEMENTED YET! 0x{:02X}", self.rom_chip[MBC_TYPE])
        };
    }

    pub fn get_rom_info(&self) -> Vec<u8> {
        let mut info = vec![];
        info.extend_from_slice(&self.rom_chip[0x134..=0x143]); // title
        info.extend_from_slice(&self.rom_chip[0x14E..=0x14F]); // global checksum
        info
    }

    pub fn read(&self, addr: u16) -> u8 {
        if self.boot_rom_mounted && addr <= 0xFF {
            return self.boot_rom[addr as usize]
        }

        match addr {
            0x0000..=0x7FFF => {
                if self.memory_bank == MemoryBank::MBC1 {
                    return self.mbc1_read(addr);
                } else if self.memory_bank == MemoryBank::MBC3 {
                    return self.mbc3_read(addr);
                };
                self.rom_chip[addr as usize]
            },
            0xA000..=0xBFFF => {
                if self.memory_bank == MemoryBank::MBC1 {
                    return self.mbc1_read(addr);
                } else if self.memory_bank == MemoryBank::MBC3 {
                    return self.mbc3_read(addr);
                }
                self.rom_chip[addr as usize]
            },
            0x8000..=0x9FFF => self.ppu.read_vram(addr - 0x8000),
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize], // 4 KiB Work RAM (WRAM)
            0xFE00..=0xFE9F => self.ppu.read_oam(addr - 0xFE00),
            0xFF00 => {
                if self.keypress != -1 {
                    let mut buttons_pressed = 0xF;
    
                    if ((self.joyp >> 4) & 0x1 == 0) && ((self.joyp >> 5) & 0x1 == 1) { // DPAD
                        buttons_pressed = match self.keypress {
                            1 => buttons_pressed & !(1 << 2), // UP
                            2 => buttons_pressed & !(1 << 1), // LEFT
                            3 => buttons_pressed & !(1 << 3), // DOWN
                            4 => buttons_pressed & !(1 << 0), // RIGHT 
                            _ => 0xF
                        };
                    } else if ((self.joyp >> 5) & 0x1 == 0) && ((self.joyp >> 4) & 0x1 == 1) { // SELECT
                        buttons_pressed = match self.keypress {
                            5 => buttons_pressed & !(1 << 0), // A
                            6 => buttons_pressed & !(1 << 1), // B
                            7 => buttons_pressed & !(1 << 3), // START
                            8 => buttons_pressed & !(1 << 2), // SELECT 
                            _ => 0xF
                        };
                    }
                    return buttons_pressed
                }
                return 0xF;
            }
            0xFF01 => 0xFF, // some serial register not implemented.
            0xFF04..=0xFF07 => self.timer.read_registers(addr),
            0xFF0F => self.IF,
            0xFF40..=0xFF4B => self.ppu.read_registers(addr),
            0xFF50 => self.boot_rom_mounted as u8,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize], // High RAM (HRAM)
            0xFFFF => self.IE,

            _ => 0x00
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => {
                if self.memory_bank == MemoryBank::MBC1 {
                    self.mbc1_write(addr, val)
                } else if self.memory_bank == MemoryBank::MBC3 {
                    self.mbc3_write(addr, val)
                }
            },
            0xA000..=0xBFFF => {
                if self.memory_bank == MemoryBank::MBC1 {
                    self.mbc1_write(addr, val)
                } else if self.memory_bank == MemoryBank::MBC3 {
                    self.mbc3_write(addr, val)
                }
            },
            0x8000..=0x9FFF => self.ppu.write_vram(addr - 0x8000, val), // 8 KiB Video RAM (VRAM)
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize] = val, // 4 KiB Work RAM (WRAM)
            0xFE00..=0xFE9F => self.ppu.write_oam(addr - 0xFE00, val), // Object attribute memory (OAM)
            0xFF00 => self.joyp = val,
            0xFF04..=0xFF07 => self.timer.write_registers(addr, val),
            0xFF0F => self.IF = val,
            0xFF46 => self.oam_dma_transfer((val as u16) << 8),
            0xFF40..=0xFF4B => self.ppu.write_registers(addr, val),
            0xFF50 => {
                if val & 0x1 == 1 { // bit 0 must be explicitly set to unmap the bootrom
                    self.boot_rom_mounted = false
                }
            },
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = val, // High RAM (HRAM)
            0xFFFF => self.IE = val,

            _ => ()
        }
    }

    fn oam_dma_transfer(&mut self, source: u16) {
        for i in 0..0xA0 {
            self.ppu.oam[i] = self.read(source + (i as u16))
        }
    }

    fn mbc1_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => {
                let offset = if self.banking_mode == BankingMode::ADVANCED { ((self.ram_rom_bank_number as u32) << 19) | ((addr as u32) & 0x3FFF) } else { addr as u32 };
                return self.rom_chip[(offset as usize) & (self.rom_chip.len() - 1)];
            },
            0x4000..=0x7FFF => {
                let translated_bank_number = if self.rom_bank_number == 0x00 { 0x01 } else { self.rom_bank_number };
                let offset = ((self.ram_rom_bank_number as u32) << 19) | ((translated_bank_number as u32) << 14) | ((addr as u32) & 0x3FFF);
                return self.rom_chip[(offset as usize) & (self.rom_chip.len() - 1)];
            },
            0xA000..=0xBFFF => {
                if self.mbc_ram_enabled {
                    let mut offset = 0;
                    if self.banking_mode == BankingMode::ADVANCED && self.rom_chip[RAM_SIZE] == 0x03 { // 32 KiB RAM carts only
                        offset = (self.ram_rom_bank_number as u16) * 0x2000;
                    }
                    return self.sram[(offset + (addr & 0x1FFF)) as usize];
                }
                return 0xFF
            },

            _ => unreachable!("should not have recieved values outside of this region.")
        }
    }

    fn mbc1_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x1FFF => self.mbc_ram_enabled = val & 0xF == 0xA,
            0x2000..=0x3FFF => self.rom_bank_number = val & 0x1F,
            0x4000..=0x5FFF => self.ram_rom_bank_number = val & 0x3,
            0x6000..=0x7FFF => self.banking_mode = if val & 0x1 == 1 { BankingMode::ADVANCED } else { BankingMode::SIMPLE },
            0xA000..=0xBFFF => {
                if self.mbc_ram_enabled {
                    let mut offset = 0;
                    if self.banking_mode == BankingMode::ADVANCED && self.rom_chip[RAM_SIZE] == 0x03 { // 32 KiB RAM carts only
                        offset = (self.ram_rom_bank_number as u16) * 0x2000;
                    }
                    self.sram[(offset + (addr & 0x1FFF)) as usize] = val;
                }
            },
            _ => unreachable!("should not have recieved values outside of this region.")
        }
    }

    fn mbc3_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom_chip[(addr & 0x3FFF) as usize],
            0x4000..=0x7FFF => {
                let offset = ((self.rom_bank_number as u32) << 14) | ((addr as u32) & 0x3FFF);
                return self.rom_chip[(offset as usize) & (self.rom_chip.len() - 1)];
            },
            0xA000..=0xBFFF => {
                if self.ram_rom_bank_number > 0x03 {
                    unreachable!("did not implemenent RTC stuff yet.")
                }

                if self.mbc_ram_enabled {
                    let mut offset = 0;
                    if self.banking_mode == BankingMode::ADVANCED && self.rom_chip[RAM_SIZE] == 0x03 { // 32 KiB RAM carts only
                        offset = (self.ram_rom_bank_number as u16) * 0x2000;
                    }
                    return self.sram[(offset + (addr & 0x1FFF)) as usize];
                }
                return 0xFF
            }, 

            _ => unreachable!("should not have recieved values outside of this region.")
        }
    }

    fn mbc3_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x1FFF => {
                if val == 0x0A {
                    self.mbc_ram_enabled = true;
                } else if val == 0x00 {
                    self.mbc_ram_enabled = false;
                }
            },
            0x2000..=0x3FFF => self.rom_bank_number = if val == 0x00 { 0x01 } else { val & 0x7F },
            0x4000..=0x5FFF => self.ram_rom_bank_number = val,
            0x6000..=0x7FFF => (), // Latch Clock Data (Write Only)
            0xA000..=0xBFFF => {
                if self.mbc_ram_enabled {
                    let mut offset = 0;
                    if self.banking_mode == BankingMode::ADVANCED && self.rom_chip[RAM_SIZE] == 0x03 { // 32 KiB RAM carts only
                        offset = (self.ram_rom_bank_number as u16) * 0x2000;
                    }
                    self.sram[(offset + (addr & 0x1FFF)) as usize] = val;
                }
            }

            _ => panic!("should not have recieved values outside of this region.")
        }
    }

    pub fn create_bess_mbc_block(&self) -> Option<Vec<u8>> {
        match self.memory_bank {
            MemoryBank::MBCNONE => None,
            MemoryBank::MBC1 => Some(vec![0x00, 0x00, if self.mbc_ram_enabled { 0x0A } else { 0x00 }, 0x00, 0x20, self.rom_bank_number, 0x00, 0x40, self.ram_rom_bank_number, 0x00, 0x60, if self.banking_mode == BankingMode::ADVANCED { 1 } else { 0 }]),
            MemoryBank::MBC3 => Some(vec![0x00, 0x00, if self.mbc_ram_enabled { 0x0A } else { 0x00 }, 0x00, 0x20, self.rom_bank_number, 0x00, 0x40, self.ram_rom_bank_number, 0x00, 0x60, 0x00, 0x00, 0xA0, 0x00]), // latch key not implemented as well as RTC register
            _ => unreachable!()
        }
    }

    pub fn aggregate_buffers(&mut self) -> Vec<u8> {
        let mut buffers = vec![];

        self.bess_buffer_offsets.extend(u32_to_little_endian(self.wram.len() as u32)); // size of wram
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // offset of wram
        buffers.extend(self.wram);

        self.bess_buffer_offsets.extend(u32_to_little_endian(self.ppu.vram.len() as u32)); // size of vram
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // offset of vram
        buffers.extend(self.ppu.vram);
        
        self.bess_buffer_offsets.extend(u32_to_little_endian(self.sram.len() as u32)); // size of sram
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // offset of sram
        buffers.extend(&self.sram);
        
        self.bess_buffer_offsets.extend(u32_to_little_endian(self.ppu.oam.len() as u32)); // size of oam
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // offset of oam
        buffers.extend(self.ppu.oam);
        
        self.bess_buffer_offsets.extend(u32_to_little_endian(self.hram.len() as u32)); // size of hram
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // offset of hram
        buffers.extend(self.hram);
        
        /* JUST DMG SO BG AND OBJ PALLETES ARE STORED IN REGISTERS NOT BUFFERS */

        // background palletes
        self.bess_buffer_offsets.extend(u32_to_little_endian(0x00));
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // ?
        
        // object palletes
        self.bess_buffer_offsets.extend(u32_to_little_endian(0x00));
        self.bess_buffer_offsets.extend(u32_to_little_endian(buffers.len() as u32)); // ?
        
        buffers
    }

    pub fn propogate_buffers(&mut self) {

    }

    pub fn update_requested_interrupts(&mut self) {
        let mut requests: u8 = 0x0;

        if !self.ppu.vblank_irq_triggered { // VBLANK interrupt
            requests |= 0b00000001; 
            self.ppu.vblank_irq_triggered = true;
        }

        if !self.ppu.stat_irq_triggered {
            if ((self.ppu.stat >> 6) & 0x1 == 1) && (self.ppu.ly == self.ppu.lyc) { requests |= 0b00000010; } // STAT interrupt (LY == LYC)
            if ((self.ppu.stat >> 5) & 0x1 == 1) && (self.ppu.stat & 0x3 == 2) { requests |= 0b00000010 }; // STAT interrupt (OAM)
            if ((self.ppu.stat >> 4) & 0x1 == 1) && (self.ppu.stat & 0x3 == 1) { requests |= 0b00000010 }; // STAT interrupt (VBLANK)
            if ((self.ppu.stat >> 3) & 0x1 == 1) && (self.ppu.stat & 0x3 == 0) { requests |= 0b00000010 }; // STAT interrupt (HBLANK)
            self.ppu.stat_irq_triggered = true;
        }

        if self.timer.tima_irq > 0 { // starts at 2 to delay 1 cycle
            self.timer.tima_irq -= 1;
            if self.timer.tima_irq == 0 { 
                requests |= 0b00000100; // TIMER interrupt
            }
        }

        self.IF |= requests;
    }

    pub fn update_components(&mut self) { // 1 cycle
        self.ppu.update();
        self.timer.update();
    }

    pub fn get_display(&self) -> Display {
        self.ppu.lcd
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            rom_chip: vec![],
            banking_mode: BankingMode::SIMPLE,
            memory_bank: MemoryBank::MBCNONE,
            mbc_ram_enabled: false,
            boot_rom: [0x0; 0x100],
            boot_rom_mounted: false,
            ppu: PPU::default(),
            IE: 0x0,
            IF: 0x0,
            joyp: 0x0,
            keypress: -1,
            timer: Timer::default(),
            flat_ram: false,
            ram_rom_bank_number: 0x00,
            rom_bank_number: 0x00,
            hram: [0x0; 0x7F],
            wram: [0x0; 0x2000],
            sram: vec![],
            apu: APU::default(),
            bess_buffer_offsets: vec![]
        }
    }
}