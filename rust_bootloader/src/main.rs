#![no_main]
#![no_std]

// Tested on weact stm32h750vbt6 board + w25q64 spi flash
use defmt::info;
use embassy_executor::Spawner;
//use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::mode::Blocking;
use embassy_stm32::qspi::enums::{
    AddressSize, ChipSelectHighTime, FIFOThresholdLevel, MemorySize, *,
};
use embassy_stm32::qspi::{Config as QspiCfg, Instance, Qspi, TransferConfig};
use embassy_stm32::time::mhz;
use embassy_stm32::Config;
//use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    // RCC config
    let mut config = Config::default();
    info!("START");
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: mhz(25),
            mode: HseMode::Oscillator,
        });
        config.rcc.hsi = None;
        config.rcc.csi = false;

        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: true,
        }); // needed for USB
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV5,
            mul: PllMul::MUL160,
            divp: Some(PllDiv::DIV2),
            divq: Some(PllDiv::DIV4),
            //divr: None,
            divr: Some(PllDiv::DIV2),
        });
        config.rcc.sys = Sysclk::PLL1_P; // 400 Mhz
        config.rcc.ahb_pre = AHBPrescaler::DIV2; // 200 Mhz
        config.rcc.apb1_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb2_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb3_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb4_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.voltage_scale = VoltageScale::Scale1;
        config.rcc.mux.usbsel = mux::Usbsel::HSI48;
    }

    // Initialize peripherals
    let p = embassy_stm32::init(config);

    let qspi_config = QspiCfg {
        memory_size: MemorySize::_8MiB,
        address_size: AddressSize::_24bit,
        prescaler: 16,
        cs_high_time: ChipSelectHighTime::_1Cycle,
        fifo_threshold: FIFOThresholdLevel::_16Bytes,
    };

    let qspi = embassy_stm32::qspi::Qspi::new_blocking_bank1(
        p.QUADSPI,
        p.PD11,
        p.PD12,
        p.PE2,
        p.PD13,
        p.PB2,
        p.PB6,
        qspi_config,
    );

    let mut flash = FlashMemory::new(qspi);

    let flash_id = flash.read_id();
    info!("FLASH ID: {=[u8]:x}", flash_id);
    // let mut wr_buf = [0u8; 8];
    // for i in 0..8 {
    //     wr_buf[i] = i as u8;
    // }
    // let mut rd_buf = [0u8; 8];
    // flash.erase_sector(0).await;
    // flash.write_memory(0, &wr_buf, true).await;
    // flash.read_memory(0, &mut rd_buf, true);
    // info!("WRITE BUF: {=[u8]:#X}", wr_buf);
    // info!("READ BUF: {=[u8]:#X}", rd_buf);
    flash.enable_mm().await;
    info!("Enabled memory mapped mode");

    // let first_u32 = unsafe { *(0x90000000 as *const u32) };
    // assert_eq!(first_u32, 0x03020100);
    // let second_u32 = unsafe { *(0x90000004 as *const u32) };
    // assert_eq!(second_u32, 0x07060504);
    //let buf = unsafe { core::slice::from_raw_parts(0x90000000 as *const u8, 8) };
    //info!("BUF: {=[u8]:#X}", buf);
    // flash.disable_mm().await;

    info!("DONE");

    // load app
    unsafe {
        //let mut p = cortex_m::Peripherals::steal();
        //cortex_m::register::control::write(cortex_m::register::control::Control::from_bits(0));
        //cortex_m::interrupt::disable();

        cortex_m::asm::bootload(0x9000_0000u32 as *const u32)
        //cortex_m::asm::bootstrap(0x9000_0000u32 as *const u32, (0x9000_0000u32 + 4) as *const u32)
    }

    // let mut led = Output::new(p.PC13, Level::Low, Speed::Low);
    // loop {
    //     led.toggle();
    //     Timer::after_millis(5000).await;
    // }
}

const MEMORY_PAGE_SIZE: usize = 8;

const CMD_QUAD_READ: u8 = 0x6B;

const CMD_QUAD_WRITE_PG: u8 = 0x32;

const CMD_READ_ID: u8 = 0x9F;

const CMD_ENABLE_RESET: u8 = 0x66;
const CMD_RESET: u8 = 0x99;

const CMD_WRITE_ENABLE: u8 = 0x06;

const CMD_CHIP_ERASE: u8 = 0xC7;
const CMD_SECTOR_ERASE: u8 = 0x20;
const CMD_BLOCK_ERASE_32K: u8 = 0x52;
const CMD_BLOCK_ERASE_64K: u8 = 0xD8;

const CMD_READ_SR: u8 = 0x05;
const CMD_READ_CR: u8 = 0x35;

const CMD_WRITE_SR: u8 = 0x01;
const CMD_WRITE_CR: u8 = 0x31;

/// Implementation of access to flash chip.
/// Chip commands are hardcoded as it depends on used chip.
/// This implementation is using chip GD25Q64C from Giga Device
pub struct FlashMemory<I: Instance> {
    qspi: Qspi<'static, I, Blocking>,
}

impl<I: Instance> FlashMemory<I> {
    pub fn new(qspi: Qspi<'static, I, Blocking>) -> Self {
        let mut memory = Self { qspi };

        memory.reset_memory();
        memory.enable_quad();
        memory
    }

    // pub async fn disable_mm(&mut self) {
    //     self.qspi.disable_memory_mapped_mode();
    // }

    pub async fn enable_mm(&mut self) {
        let transaction: TransferConfig = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::SING,
            dwidth: QspiWidth::QUAD,
            instruction: CMD_QUAD_READ,
            address: Some(0),
            dummy: DummyCycles::_8,
        };

        self.qspi.enable_memory_map(&transaction);
    }

    fn enable_quad(&mut self) {
        let cr = self.read_cr();
        // info!("Read cr: {:x}", cr);
        self.write_cr(cr | 0x02);
        // info!("Read cr after writing: {:x}", cr);
    }

    pub fn disable_quad(&mut self) {
        let cr = self.read_cr();
        self.write_cr(cr & (!(0x02)));
    }

    fn exec_command_4(&mut self, cmd: u8) {
        let transaction = TransferConfig {
            iwidth: QspiWidth::QUAD,
            awidth: QspiWidth::NONE,
            // adsize: AddressSize::_24bit,
            dwidth: QspiWidth::NONE,
            instruction: cmd,
            address: None,
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        self.qspi.blocking_command(transaction);
    }

    fn exec_command(&mut self, cmd: u8) {
        let transaction = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::NONE,
            dwidth: QspiWidth::NONE,
            instruction: cmd,
            address: None,
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        // info!("Excuting command: {:x}", transaction.instruction);
        self.qspi.blocking_command(transaction);
    }

    pub fn reset_memory(&mut self) {
        self.exec_command_4(CMD_ENABLE_RESET);
        self.exec_command_4(CMD_RESET);
        self.exec_command(CMD_ENABLE_RESET);
        self.exec_command(CMD_RESET);
        self.wait_write_finish();
    }

    pub fn enable_write(&mut self) {
        self.exec_command(CMD_WRITE_ENABLE);
    }

    pub fn read_id(&mut self) -> [u8; 3] {
        let mut buffer = [0; 3];
        let transaction: TransferConfig = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::NONE,
            // adsize: AddressSize::_24bit,
            dwidth: QspiWidth::SING,
            instruction: CMD_READ_ID,
            ..Default::default()
        };
        // info!("Reading id: 0x{:X}", transaction.instruction);
        self.qspi.blocking_read(&mut buffer, transaction);
        buffer
    }

    // pub fn read_id_4(&mut self) -> [u8; 3] {
    //     let mut buffer = [0; 3];
    //     let transaction: TransferConfig = TransferConfig {
    //         iwidth: QspiWidth::SING,
    //         isize: AddressSize::_8Bit,
    //         adwidth: QspiWidth::NONE,
    //         dwidth: QspiWidth::QUAD,
    //         instruction: Some(CMD_READ_ID as u32),
    //         ..Default::default()
    //     };
    //     info!("Reading id: 0x{:X}", transaction.instruction);
    //     self.qspi.blocking_read(&mut buffer, transaction).unwrap();
    //     buffer
    // }

    pub fn read_memory(&mut self, addr: u32, buffer: &mut [u8], _use_dma: bool) {
        let transaction = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::SING,
            //adsize: AddressSize::_24bit,
            dwidth: QspiWidth::QUAD,
            instruction: CMD_QUAD_READ,
            address: Some(addr),
            dummy: DummyCycles::_8,
            //..Default::default()
        };
        self.qspi.blocking_read(buffer, transaction);
    }

    fn wait_write_finish(&mut self) {
        while (self.read_sr() & 0x01) != 0 {}
    }

    async fn perform_erase(&mut self, addr: u32, cmd: u8) {
        let transaction = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::SING,
            dwidth: QspiWidth::NONE,
            instruction: cmd,
            address: Some(addr),
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        self.enable_write();
        self.qspi.blocking_command(transaction);
        self.wait_write_finish();
    }

    pub async fn erase_sector(&mut self, addr: u32) {
        self.perform_erase(addr, CMD_SECTOR_ERASE).await;
    }

    pub async fn erase_block_32k(&mut self, addr: u32) {
        self.perform_erase(addr, CMD_BLOCK_ERASE_32K).await;
    }

    pub async fn erase_block_64k(&mut self, addr: u32) {
        self.perform_erase(addr, CMD_BLOCK_ERASE_64K).await;
    }

    pub async fn erase_chip(&mut self) {
        self.exec_command(CMD_CHIP_ERASE);
    }

    async fn write_page(&mut self, addr: u32, buffer: &[u8], len: usize, _use_dma: bool) {
        assert!(
            (len as u32 + (addr & 0x000000ff)) <= MEMORY_PAGE_SIZE as u32,
            "write_page(): page write length exceeds page boundary (len = {}, addr = {:X}",
            len,
            addr
        );

        let transaction = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::SING,
            dwidth: QspiWidth::QUAD,
            instruction: CMD_QUAD_WRITE_PG,
            address: Some(addr),
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        self.enable_write();
        self.qspi.blocking_write(buffer, transaction);
        self.wait_write_finish();
    }

    pub async fn write_memory(&mut self, addr: u32, buffer: &[u8], use_dma: bool) {
        let mut left = buffer.len();
        let mut place = addr;
        let mut chunk_start = 0;

        while left > 0 {
            let max_chunk_size = MEMORY_PAGE_SIZE - (place & 0x000000ff) as usize;
            let chunk_size = if left >= max_chunk_size {
                max_chunk_size
            } else {
                left
            };
            let chunk = &buffer[chunk_start..(chunk_start + chunk_size)];
            self.write_page(place, chunk, chunk_size, use_dma).await;
            place += chunk_size as u32;
            left -= chunk_size;
            chunk_start += chunk_size;
        }
    }

    fn read_register(&mut self, cmd: u8) -> u8 {
        let mut buffer = [0; 1];
        let transaction: TransferConfig = TransferConfig {
            iwidth: QspiWidth::SING,
            awidth: QspiWidth::NONE,
            dwidth: QspiWidth::SING,
            instruction: cmd,
            address: None,
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        self.qspi.blocking_read(&mut buffer, transaction);
        // info!("Read w25q64 register: 0x{:x}", buffer[0]);
        buffer[0]
    }

    fn write_register(&mut self, cmd: u8, value: u8) {
        let buffer = [value; 1];
        let transaction: TransferConfig = TransferConfig {
            iwidth: QspiWidth::SING,
            instruction: cmd,
            awidth: QspiWidth::NONE,
            dwidth: QspiWidth::SING,
            address: None,
            dummy: DummyCycles::_0,
            //..Default::default()
        };
        self.qspi.blocking_write(&buffer, transaction);
    }

    pub fn read_sr(&mut self) -> u8 {
        self.read_register(CMD_READ_SR)
    }

    pub fn read_cr(&mut self) -> u8 {
        self.read_register(CMD_READ_CR)
    }

    pub fn write_sr(&mut self, value: u8) {
        self.write_register(CMD_WRITE_SR, value);
    }

    pub fn write_cr(&mut self, value: u8) {
        self.write_register(CMD_WRITE_CR, value);
    }
}
