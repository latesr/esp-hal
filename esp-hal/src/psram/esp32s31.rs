//! PSRAM driver for ESP32-S31.

use super::{EXTMEM_ORIGIN, PsramSize};
use crate::{
    clock::ll::{ClockTree, PsramFunctionClockConfig, PsramInstance},
    peripherals::HP_SYS_CLKRST,
};

mod oct_hex;

pub(crate) use oct_hex::low_speed;

/// PSRAM configuration.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[instability::unstable]
pub struct PsramConfig {
    /// Size of PSRAM to map. Default: `AutoDetect` via MR2 density.
    pub size: PsramSize,

    /// PSRAM timing parameters. Default: 200 MHz. 250 MHz needs the MPLL at 500 MHz.
    ///
    /// With `code-in-psram`, PSRAM runs at 200 MHz from start-up, and this field is ignored. For
    /// 250 MHz, set `ClockConfig::mpll_clk` to 500 MHz in the config of `esp_hal::init`.
    pub timing: PsramTimingParams,
    // TODO: ECC enable.
    // The MSPI0 controller has a ECC engine.
    // pub ecc: bool, // or any other enum.
}

/// PSRAM timing parameters.
///
/// These values should be tuned together with the source clock frequency. The source clock
/// frequency must be an integer multiple of the PSRAM frequency.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PsramTimingParams {
    /// PSRAM clock source.
    pub clock_source: PsramFunctionClockConfig,

    /// Bus clock frequency in MHz. Must divide the source clock evenly.
    pub clock: u32,

    /// MR0.read_latency field value (cycles = 2 * value + 6).
    pub mr0_rl: u8,

    /// MR4.wr_latency field value.
    pub mr4_wl: u8,

    /// Reads dummy length in bits for sync data reads (cache path).
    ///
    /// For `N` dummy bits, configure `reg_dummy_bits` to `N - 1`.
    pub rd_dummy_bits: u8,

    /// Writes dummy length in bits for sync data writes (cache path).
    ///
    /// For `N` dummy bits, configure `reg_dummy_bits` to `N - 1`.
    pub wr_dummy_bits: u8,

    /// Register-read dummy length for direct command path (MSPI3).
    pub reg_dummy_bits: u32,
}

impl Default for PsramTimingParams {
    fn default() -> Self {
        Self::MHZ_200
    }
}

impl PsramTimingParams {
    /// Preset for 20 MHz clock speed.
    pub const MHZ_20: Self = Self {
        clock_source: PsramFunctionClockConfig::Mpll,
        clock: 20,
        mr0_rl: 2,
        mr4_wl: 2,
        rd_dummy_bits: 17,
        wr_dummy_bits: 7,
        reg_dummy_bits: 8,
    };

    /// Preset for 80 MHz clock speed.
    pub const MHZ_80: Self = Self {
        clock_source: PsramFunctionClockConfig::Mpll,
        clock: 80,
        mr0_rl: 2,
        mr4_wl: 2,
        rd_dummy_bits: 17,
        wr_dummy_bits: 7,
        reg_dummy_bits: 8,
    };

    /// Preset for 125 MHz clock speed.
    pub const MHZ_125: Self = Self {
        clock_source: PsramFunctionClockConfig::Mpll,
        clock: 125,
        mr0_rl: 2,
        mr4_wl: 2,
        rd_dummy_bits: 17,
        wr_dummy_bits: 7,
        reg_dummy_bits: 8,
    };

    /// Preset for 200 MHz clock speed.
    pub const MHZ_200: Self = Self {
        clock_source: PsramFunctionClockConfig::Mpll,
        clock: 200,
        mr0_rl: 4,
        mr4_wl: 1,
        rd_dummy_bits: 25,
        wr_dummy_bits: 11,
        reg_dummy_bits: 12,
    };

    /// Preset for 250 MHz clock speed.
    pub const MHZ_250: Self = Self {
        clock_source: PsramFunctionClockConfig::Mpll,
        clock: 250,
        mr0_rl: 6,
        mr4_wl: 3,
        rd_dummy_bits: 33,
        wr_dummy_bits: 15,
        reg_dummy_bits: 16,
    };
}

/// Initializes PSRAM.
#[cfg(not(code_in_psram))]
#[crate::ram]
pub(crate) fn init_psram(config: &mut PsramConfig) -> bool {
    // Module clock + clock source
    enable_psram_mspi();
    reset_psram_mspi();

    ClockTree::with(|clocks| {
        PsramInstance::Psram.configure_function_clock(clocks, config.timing.clock_source);
        PsramInstance::Psram.request_function_clock(clocks);
    });

    // Controller + PHY pad bring-up.
    if !oct_hex::set_bus_clock(config.timing.clock) {
        return false;
    }
    start_chip(&config.timing);

    if config.size.is_auto() {
        config.size = PsramSize::Size(oct_hex::psram_detect_size(&config.timing));
    }

    // basic AXI configuration here
    oct_hex::configure_psram_mspi(&config.timing, false);
    oct_hex::mmu_map_psram(config.size.get());

    true
}

/// Brings up the controller's DLL and pads, and the PSRAM chip at `timing`, once the bus clock is
/// set.
#[inline(always)]
fn start_chip(timing: &PsramTimingParams) {
    oct_hex::enable_dll();
    oct_hex::psram_pad_init(false); // required for DDR strobe latch
    oct_hex::set_cs_timing();

    // SoC MR init (via MSPI3 SPI direct)
    oct_hex::init_mr_registers(timing, false);
}

#[cfg(not(code_in_psram))]
pub(crate) fn map_psram(config: PsramConfig) -> core::ops::Range<usize> {
    let start = EXTMEM_ORIGIN;
    start..start + config.size.get()
}

#[cfg_attr(code_in_psram, inline(always))]
fn enable_psram_mspi() {
    HP_SYS_CLKRST::regs()
        .psram_ctrl0()
        .modify(|_, w| w.sys_clk_en().set_bit());
}

#[cfg_attr(code_in_psram, inline(always))]
fn reset_psram_mspi() {
    HP_SYS_CLKRST::regs().psram_ctrl0().modify(|_, w| {
        w.axi_rst_en().set_bit();
        w.apb_rst_en().set_bit()
    });
    HP_SYS_CLKRST::regs().psram_ctrl0().modify(|_, w| {
        w.axi_rst_en().clear_bit();
        w.apb_rst_en().clear_bit()
    });
}

#[cfg(code_in_psram)]
pub(crate) use start::*;

/// Code in PSRAM: the ESP32-S31's parts of the start-up in [`super::code_in_psram`].
///
/// Neither `.text` nor `.rodata` is in PSRAM while the start-up runs, so every function here
/// except `request_clock` and `retime_to_500mhz` must be inlined into `riscv_preinit`.
/// `esp_hal::init` calls these two functions.
#[cfg(code_in_psram)]
mod start {
    use super::*;
    use crate::{
        peripherals::{MEMSPI2, SPI0},
        psram::code_in_psram::stop,
    };

    /// Gives PSRAM's clock to the clock tree, which then keeps it on. The start-up configured the
    /// clock without the tree.
    pub(crate) fn request_clock() {
        ClockTree::with(|clocks| {
            PsramInstance::Psram.configure_function_clock(clocks, PsramFunctionClockConfig::Mpll);
            PsramInstance::Psram.request_function_clock(clocks);
        });
    }

    const PSRAM_PAGE: u32 = 0x1_0000;
    pub(crate) const FLASH_WINDOW: u32 = 0x4000_0000;
    pub(crate) const FLASH_VALID: u32 = 1 << 12;
    pub(crate) const FLASH_SENSITIVE: u32 = 1 << 13;
    pub(crate) const FLASH_PADDR: u32 = 0x7ff;
    pub(crate) const IMAGE_CHIP_ID: u32 = 0x0020;
    const ICACHES: u32 = 0b11;
    const DCACHE: u32 = 1 << 4;

    /// The start-up's MPLL rate, for PSRAM at 200 MHz. `ClockConfig::mpll_clk` can move it to
    /// 500 MHz in `esp_hal::init`, see [`retime_to_500mhz`].
    const MPLL_MHZ: u32 = 400;

    unsafe extern "C" {
        fn Cache_WriteBack_Addr(map: u32, addr: u32, size: u32);
    }

    #[inline(always)]
    pub(crate) fn flash_entry(i: u32) -> u32 {
        SPI0::regs()
            .mmu_item_index()
            .write(|w| unsafe { w.bits(i) });
        SPI0::regs().mmu_item_content().read().bits()
    }

    #[inline(always)]
    pub(crate) fn set_flash_entry(i: u32, content: u32) {
        SPI0::regs()
            .mmu_item_index()
            .write(|w| unsafe { w.bits(i) });
        SPI0::regs()
            .mmu_item_content()
            .write(|w| unsafe { w.bits(content) });
    }

    #[inline(always)]
    pub(crate) fn flash_page_shift() -> u32 {
        // Code 0 is 256 KiB, and each code above halves it.
        // See <https://github.com/espressif/esp-idf/blob/383dda6c182/components/esp_hal_cache/esp32s31/include/hal/mmu_ll.h#L125-L147>.
        let code = SPI0::regs().mmu_power_ctrl().read().mmu_page_size().bits() as u32;
        18u32.wrapping_sub(code)
    }

    #[inline(always)]
    pub(crate) fn efuse_block0() -> u32 {
        crate::peripherals::EFUSE::regs().rd_wr_dis().as_ptr() as u32
    }

    // The write-back runs twice, as in the ESP-IDF patch of `Cache_WriteBack_Addr`.
    // See <https://github.com/espressif/esp-idf/blob/383dda6c182/components/esp_rom/patches/esp_rom_cache_writeback_esp32p4_esp32s31.c#L46-L50>.
    #[inline(always)]
    pub(crate) fn cache_writeback(addr: u32, size: u32) {
        unsafe {
            Cache_WriteBack_Addr(DCACHE, addr, size);
            Cache_WriteBack_Addr(DCACHE, addr, size);
        }
    }

    #[inline(always)]
    pub(crate) fn cache_invalidate(addr: u32, size: u32) {
        crate::soc::invalidate_cache(ICACHES | DCACHE, addr, size);
    }

    /// Invalidates the instruction caches over `addr..addr + size`. The data cache lines of the
    /// copy are already written back, so only the instruction caches can hold stale code.
    #[inline(always)]
    pub(crate) fn cache_invalidate_code(addr: u32, size: u32) {
        crate::soc::invalidate_cache(ICACHES, addr, size);
    }

    #[inline(always)]
    fn hart_id() -> u32 {
        let id: u32;
        unsafe { core::arch::asm!("csrr {0}, mhartid", out(reg) id) };
        id
    }

    /// Holds core 1 in reset when the start-up runs on core 0. Core 1 waits in ROM and must not
    /// run before the code is in PSRAM. The ROM can enter on either core, as in `pre_init`.
    #[inline(always)]
    pub(crate) fn park_other_core() {
        if hart_id() == 0 {
            HP_SYS_CLKRST::regs().hpcore1_ctrl0().modify(|_, w| {
                w.core1_cpu_clk_en().clear_bit();
                w.core1_clic_clk_en().clear_bit();
                w.core1_global_rst_en().set_bit()
            });
        }
    }

    // `retime_to_500mhz` sets the bus clock without a check, so 250 MHz must divide 500 MHz.
    const _: () = core::assert!(500 % PsramTimingParams::MHZ_250.clock == 0);

    /// Moves the running PSRAM from 200 MHz to 250 MHz, and the MPLL from 400 MHz to 500 MHz.
    ///
    /// The code runs from PSRAM, so the function runs from RAM with the caches suspended. Core 1 is
    /// still in reset and nothing uses PLL_F25M or PLL_F50M yet, so it masks only this core's
    /// interrupts. The latencies change first, at 200 MHz, where the longer latencies also work.
    /// The DLL gets no settle time after the clock returns, as in ESP-IDF's light-sleep wake.
    /// See <https://github.com/espressif/esp-idf/blob/383dda6c182/components/esp_hw_support/port/esp32s31/pmu_sleep.c#L417-L428>.
    #[crate::ram(unstable(checked))]
    pub(crate) fn retime_to_500mhz() {
        let old = PsramTimingParams::MHZ_200;
        let new = PsramTimingParams::MHZ_250;
        let token =
            unsafe { esp_sync::raw::RawLock::enter(&esp_sync::raw::SingleCoreInterruptLock) };
        {
            let _caches = crate::soc::cache::CacheGuard::suspend();
            // Bits 26–31 are the six FIFO-empty bits.
            const ALL_TRANSACTION_DONE: u32 = 0xfc00_0000;
            while MEMSPI2::regs().smem_axi_addr_ctrl().read().bits() != ALL_TRANSACTION_DONE {}

            oct_hex::retime_mr_registers(&old, &new);
            oct_hex::configure_psram_mspi(&new, false);

            HP_SYS_CLKRST::regs().psram_ctrl0().modify(|_, w| unsafe {
                w.clk_src_sel().bits(0);
                w.core_clk_en().clear_bit();
                w.pll_clk_en().clear_bit()
            });
            crate::soc::clocks::set_mpll_dividers(500);
            crate::soc::clocks::set_mpll_power(false);
            crate::soc::clocks::set_mpll_power(true);
            crate::soc::clocks::set_mpll_frequency(500);
            // Cannot fail: the assertion above the function checks the divider.
            let _ = oct_hex::set_bus_clock_from(500, new.clock);
            HP_SYS_CLKRST::regs().psram_ctrl0().modify(|_, w| unsafe {
                w.clk_src_sel().bits(1);
                w.pll_clk_en().set_bit();
                w.core_clk_en().set_bit()
            });
        }
        unsafe { esp_sync::raw::RawLock::exit(&esp_sync::raw::SingleCoreInterruptLock, token) };
    }

    /// Starts the MPLL and PSRAM, maps PSRAM page i at window page i, and returns its size.
    #[inline(always)]
    pub(crate) fn start_psram() -> usize {
        crate::soc::clocks::start_mpll(MPLL_MHZ);
        enable_psram_mspi();
        reset_psram_mspi();
        HP_SYS_CLKRST::regs().psram_ctrl0().modify(|_, w| unsafe {
            w.clk_src_sel().bits(1);
            w.pll_clk_en().set_bit();
            w.core_clk_en().set_bit()
        });
        let timing = PsramTimingParams::MHZ_200;
        if !oct_hex::set_bus_clock_from(MPLL_MHZ, timing.clock) {
            stop(0x101);
        }
        super::start_chip(&timing);
        let Ok(size) = oct_hex::try_psram_detect_size(&timing) else {
            stop(0x102);
        };
        oct_hex::configure_psram_mspi(&timing, false);

        // The PSRAM MMU's code 0 is 64 KiB, and the flash MMU's is 256 KiB.
        // See <https://github.com/espressif/esp-idf/blob/383dda6c182/components/esp_hal_cache/esp32s31/include/hal/mmu_ll.h#L125-L147>.
        MEMSPI2::regs()
            .mmu_power_ctrl()
            .modify(|_, w| unsafe { w.mmu_page_size().bits(0) });
        let mut page = 0;
        while page < size as u32 / PSRAM_PAGE {
            oct_hex::write_psram_mmu_entry(page, page as u16);
            page = page.wrapping_add(1);
        }
        crate::soc::invalidate_cache(DCACHE, EXTMEM_ORIGIN as u32, size as u32);
        size
    }
}
