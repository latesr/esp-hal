//! Code in PSRAM: the firmware's code and read-only data run from the PSRAM window.

//% CHIP_FILTER: psram_supports_code_in_psram
//% ENV: ESP_HAL_CONFIG_CODE_IN_PSRAM=true
//% FEATURES: unstable

#![no_std]
#![no_main]

use core::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};

use critical_section::Mutex;
use esp_hal::interrupt::software::SoftwareInterrupt;
use hil_test as _;

#[macro_use]
extern crate esp_metadata_generated;

static RODATA: [u32; 4] = [0x5053_524D; 4];
static mut DATA: u32 = 0x4441_5441;
static mut BSS: u32 = 0;

static SWINT: Mutex<RefCell<Option<SoftwareInterrupt<'static, 2>>>> =
    Mutex::new(RefCell::new(None));
static INTERRUPTS: AtomicU32 = AtomicU32::new(0);

// 256 KiB, so the static spans several MMU pages.
#[esp_hal::psram(zeroed)]
static mut PSRAM_BSS: [u32; 0x1_0000] = [0; 0x1_0000];

// Not `mut`: the attribute accepts a static with interior mutability.
#[esp_hal::psram(zeroed)]
static PSRAM_WAKER: Mutex<RefCell<Option<core::task::Waker>>> = Mutex::new(RefCell::new(None));

#[repr(C, align(64))]
struct DmaBlock([u8; 4096]);

#[esp_hal::psram(zeroed)]
static mut DMA_SOURCE: DmaBlock = DmaBlock([0; 4096]);

unsafe extern "C" {
    static _psram_code_end: u8;
    static _psram_bss_end: u8;
}

#[inline(never)]
fn in_text() -> u32 {
    unsafe { core::ptr::read_volatile(&RODATA[0]) }
}

extern "C" fn swint_handler() {
    INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    critical_section::with(|cs| {
        if let Some(swint) = SWINT.borrow_ref(cs).as_ref() {
            swint.reset();
        }
    });
}

/// Calls `f` with the load address, the length, and the flash offset of the data of each segment
/// of the booted image. Returns the offset and the length of the booted partition, and the flash
/// offset after the last segment.
fn booted_image(
    flash: &mut esp_hal::flash::Flash<'_, esp_hal::Blocking>,
    mut f: impl FnMut(&mut esp_hal::flash::Flash<'_, esp_hal::Blocking>, u32, u32, u32),
) -> (u32, u32, u32) {
    use esp_bootloader_esp_idf::partitions;

    let mut pt_mem = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
    let pt = partitions::read_partition_table(flash, &mut pt_mem).unwrap();
    let app = pt.booted_partition().unwrap().unwrap();
    let (app, app_len) = (app.offset(), app.len());

    // ESP-IDF image: a 24-byte header, then segments of load address, length, and data.
    let mut header = [0u32; 6];
    flash.read(app, &mut header).unwrap();
    assert_eq!(header[0] & 0xFF, 0xE9);
    let mut offset = app + 24;
    for _ in 0..(header[0] >> 8) & 0xFF {
        let mut segment = [0u32; 2];
        flash.read(offset, &mut segment).unwrap();
        let [load, len] = segment;
        f(flash, load, len, offset + 8);
        offset += 8 + len;
    }
    (app, app_len, offset)
}

/// Returns the flash offset of a random 64 KiB block after the image in the booted partition. A
/// random block spreads the wear of the tests over the partition.
fn scratch_block(flash: &mut esp_hal::flash::Flash<'_, esp_hal::Blocking>) -> u32 {
    const BLOCK: u32 = 0x1_0000;
    // The checksum and the hash after the last segment fit in the next block.
    let (app, app_len, end) = booted_image(flash, |_, _, _, _| {});
    let first = (end + BLOCK).div_ceil(BLOCK);
    let last = (app + app_len) / BLOCK;
    assert!(
        first < last,
        "no free 64 KiB block after the image in the booted partition"
    );
    (first + esp_hal::rng::Rng::new().random() % (last - first)) * BLOCK
}

/// The number of `u32` words in 256 KiB, the size of the PSRAM data that the tests write and read
/// back. It is four times the largest data cache of these chips, 64 KiB on the ESP32-S31, so most
/// of it reaches PSRAM, and most of the read-back comes from PSRAM, not from the cache.
const UNCACHED_WORDS: usize = 0x1_0000;

/// The word at `i` of the pattern for `seed`. It differs from word to word and from seed to seed.
fn pattern_word(seed: u32, i: usize) -> u32 {
    (i as u32 ^ seed).wrapping_mul(0x9E37_79B9) ^ seed
}

/// A seed for [`fill_pattern`] that differs from run to run. PSRAM keeps its content across the
/// reset between two tests, so a fixed seed could find the last run's pattern still there.
fn fresh_seed() -> u32 {
    esp_hal::rng::Rng::new().random()
}

fn fill_pattern(words: &mut [u32], seed: u32) {
    for (i, word) in words.iter_mut().enumerate() {
        *word = pattern_word(seed, i);
    }
}

/// Asserts that `words` hold the pattern for `seed`, and names the first word that does not.
fn assert_pattern(words: &[u32], seed: u32, what: &str) {
    let first_difference = words
        .iter()
        .enumerate()
        .position(|(i, &word)| word != pattern_word(seed, i));
    assert_eq!(first_difference, None, "{what}: the pattern differs");
}

#[embedded_test::tests(default_timeout = 5)]
mod tests {
    use core::ptr::{addr_of, addr_of_mut};

    #[cfg(feature = "esp32s31")]
    use esp_hal::{
        clock::{
            ClockConfig,
            ll::{ClockTree, MpllClkConfig, mpll_clk_frequency, request_mpll_clk},
        },
        psram::PsramTimingParams,
    };
    use esp_hal::{
        flash::{Config, Flash},
        interrupt::{InterruptHandler, Priority},
        psram::{Psram, PsramConfig},
    };

    use super::*;

    const WINDOW: u32 = property!("psram.extmem_origin");
    const WINDOW_SIZE: u32 = property!("mmu.entry_num") * property!("mmu.page_size");
    const PATTERN: u32 = 0xA5C3_5A3C;

    fn in_window(addr: u32) -> bool {
        (WINDOW..WINDOW + WINDOW_SIZE).contains(&addr)
    }

    #[test]
    fn test_code_and_rodata_in_psram() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());

        assert!(in_window(in_text as *const () as u32));
        assert!(in_window(addr_of!(RODATA) as u32));
        assert_eq!(in_text(), 0x5053_524D);
    }

    #[test]
    fn test_data_and_bss_are_initialized() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());

        assert_eq!(unsafe { addr_of!(DATA).read_volatile() }, 0x4441_5441);
        assert_eq!(unsafe { addr_of!(BSS).read_volatile() }, 0);
    }

    // Reads go through the data cache, so this test shows the content and bounds of the copy, not
    // the write-back.
    #[test]
    #[timeout(30)]
    fn test_segments_match_image() {
        let peripherals = esp_hal::init(esp_hal::Config::default());
        let mut flash = Flash::new(peripherals.FLASH, Config::default()).unwrap();

        let mut compared = 0;
        booted_image(&mut flash, |flash, load, len, data| {
            if !in_window(load) {
                return;
            }
            let mut chunk = [0u32; 256];
            let mut done = 0;
            while done < len {
                let words = ((len - done) / 4).min(chunk.len() as u32) as usize;
                flash.read(data + done, &mut chunk[..words]).unwrap();
                let mapped =
                    unsafe { core::slice::from_raw_parts((load + done) as *const u32, words) };
                let first_difference = chunk[..words].iter().zip(mapped).position(|(a, b)| a != b);
                assert_eq!(first_difference, None, "in the 1 KiB at {:#x}", load + done);
                done += words as u32 * 4;
            }
            compared += len;
        });
        assert!(compared > 0);
    }

    #[test]
    fn test_heap_after_code() {
        let peripherals = esp_hal::init(esp_hal::Config::default());
        let psram = Psram::new(peripherals.PSRAM, PsramConfig::default());

        let (start, size) = psram.raw_parts();
        // The heap starts at the page after the code and the statics, as the linker placed them.
        let used = (addr_of!(_psram_code_end) as u32).max(addr_of!(_psram_bss_end) as u32);
        assert_eq!(
            start as u32,
            used.next_multiple_of(property!("mmu.page_size"))
        );

        let words = size / 4;
        let heap = unsafe { core::slice::from_raw_parts_mut(start as *mut u32, words) };
        let probes = [0, words / 2, words - 1];
        for (n, &i) in probes.iter().enumerate() {
            heap[i] = PATTERN.wrapping_add(n as u32);
        }
        for (n, &i) in probes.iter().enumerate() {
            assert_eq!(heap[i], PATTERN.wrapping_add(n as u32));
        }
    }

    /// Asserts that `PSRAM_BSS` is all zero and above the code, then writes a pattern over all of
    /// it.
    ///
    /// PSRAM keeps its content across the reset between two tests. The static is larger than the
    /// cache, so most of the pattern is in PSRAM at the reset, and the second test that calls this
    /// reads zeros only if the start-up zeroed the static.
    fn assert_psram_bss_zeroed_then_write() {
        let start = addr_of!(PSRAM_BSS) as u32;
        assert!(in_window(start));
        assert!(start >= addr_of!(_psram_code_end) as u32);

        let words = unsafe { &mut *addr_of_mut!(PSRAM_BSS) };
        assert_eq!(words.iter().position(|&w| w != 0), None);
        fill_pattern(words, PATTERN);
    }

    #[test]
    fn test_psram_static_is_zeroed_above_the_code() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());
        assert_psram_bss_zeroed_then_write();
        assert!(critical_section::with(|cs| PSRAM_WAKER
            .borrow_ref(cs)
            .is_none()));
    }

    #[test]
    fn test_psram_static_is_zeroed_again() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());
        assert_psram_bss_zeroed_then_write();
    }

    // The CPU writes the source through the cache just before the transfer, so the DMA reads the
    // written bytes only if the driver treats the static as PSRAM and writes the cache back.
    // Outside `psram_range`, the driver rejects the buffer.
    #[cfg(all(dma_can_access_psram, dma_supports_mem2mem))]
    #[test]
    fn test_dma_reads_psram_static() {
        use esp_hal::dma::{BurstConfig, ExternalBurstConfig, Mem2Mem};

        let peripherals = esp_hal::init(esp_hal::Config::default());
        let mem2mem = cfg_select! {
            feature = "esp32s31" => Mem2Mem::new(peripherals.DMA_AXI_CH1),
            _ => Mem2Mem::new(peripherals.DMA_CH0),
        };
        let (rx_buffer, rx_descriptors, _, tx_descriptors) = esp_hal::dma_buffers!(4096);
        let mut mem2mem = mem2mem
            .with_descriptors(
                rx_descriptors,
                tx_descriptors,
                BurstConfig {
                    external_memory: ExternalBurstConfig::Size64,
                    internal_memory: Default::default(),
                },
            )
            .unwrap();

        let source = unsafe { &mut (*addr_of_mut!(DMA_SOURCE)).0 };
        assert!(in_window(source.as_ptr() as u32));
        for (i, byte) in source.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(31).wrapping_add(7);
        }

        mem2mem
            .start_transfer(rx_buffer, source)
            .unwrap()
            .wait()
            .unwrap();

        for (i, &byte) in rx_buffer.iter().enumerate() {
            assert_eq!(byte, (i as u8).wrapping_mul(31).wrapping_add(7));
        }
    }

    // The heap and a PSRAM static keep their content over a flash erase, a write and a read.
    #[test]
    fn test_psram_survives_flash_operations() {
        const BLOCK: u32 = 0x1_0000;

        let peripherals = esp_hal::init(esp_hal::Config::default());
        let psram = Psram::new(peripherals.PSRAM, PsramConfig::default());
        let mut flash = Flash::new(peripherals.FLASH, Config::default()).unwrap();
        let block = scratch_block(&mut flash);

        let (start, _) = psram.raw_parts();
        let heap = unsafe { core::slice::from_raw_parts_mut(start as *mut u32, UNCACHED_WORDS) };
        let seed = fresh_seed();
        fill_pattern(heap, seed);
        let words = unsafe { &mut *addr_of_mut!(PSRAM_BSS) };
        fill_pattern(words, PATTERN);

        let data = [PATTERN; 64];
        unsafe {
            flash.erase(block, block + BLOCK).unwrap();
            flash.write(block, &data).unwrap();
        }
        let mut buffer = [0u32; 4096];
        flash.read(block, &mut buffer).unwrap();

        assert_pattern(heap, seed, "the heap after the flash operations");
        assert_pattern(words, PATTERN, "PSRAM_BSS after the flash operations");
    }

    #[test]
    fn test_interrupt_handler_in_psram() {
        let peripherals = esp_hal::init(esp_hal::Config::default());

        let mut swint = SoftwareInterrupt::new(peripherals.FROM_CPU_INTR2);
        critical_section::with(|cs| {
            swint.set_interrupt_handler(InterruptHandler::new(swint_handler, Priority::Priority1));
            SWINT.borrow_ref_mut(cs).replace(swint);
        });
        critical_section::with(|cs| SWINT.borrow_ref(cs).as_ref().unwrap().raise());

        while INTERRUPTS.load(Ordering::Relaxed) == 0 {}
        assert!(in_window(swint_handler as *const () as u32));
    }

    /// The MPLL rate that the hardware is set to, from the ESP32-S31's MPLL feedback divider, with
    /// the 40 MHz crystal and the reference divider of 2 that the start-up uses.
    #[cfg(feature = "esp32s31")]
    fn mpll_in_hardware() -> u32 {
        let fb_div = esp_hal::peripherals::LP_AON_CLK_RST::regs()
            .mspi_div()
            .read()
            .mspi_fb_div()
            .bits();
        (u32::from(fb_div) + 1) * 40_000_000 / 2
    }

    // The start-up runs PSRAM at 200 MHz. A configured 500 MHz MPLL moves PSRAM to 250 MHz in
    // `init`, while the code runs from PSRAM. The code, the heap, and a flash erase, write, and
    // read must work at the new rate.
    #[cfg(feature = "esp32s31")]
    #[test]
    fn test_mpll_rate_500mhz() {
        let mut clocks = ClockConfig::from(esp_hal::clock::CpuClock::default());
        clocks.mpll_clk = Some(MpllClkConfig::_500);
        let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(clocks));

        assert_eq!(
            ClockTree::with(|clocks| clocks.mpll_clk()),
            Some(MpllClkConfig::_500)
        );
        assert_eq!(mpll_clk_frequency(), 500_000_000);
        assert_eq!(mpll_in_hardware(), 500_000_000);
        let psram = Psram::new(peripherals.PSRAM, PsramConfig::default());
        let (start, _) = psram.raw_parts();
        let heap = unsafe { core::slice::from_raw_parts_mut(start as *mut u32, UNCACHED_WORDS) };
        let seed = fresh_seed();
        fill_pattern(heap, seed);
        assert_pattern(heap, seed, "the heap at 250 MHz");
        assert_eq!(in_text(), 0x5053_524D);

        let mut flash = Flash::new(peripherals.FLASH, Config::default()).unwrap();
        let sector = scratch_block(&mut flash);
        let mut written = [0u32; 1024];
        fill_pattern(&mut written, !seed);
        // SAFETY: the sector is the first of a block in the booted partition, after the image.
        unsafe {
            flash.erase(sector, sector + Flash::SECTOR_SIZE).unwrap();
            flash.write(sector, &written).unwrap();
        }
        let mut read_back = [0u32; 1024];
        flash.read(sector, &mut read_back).unwrap();

        assert_pattern(&read_back, !seed, "the flash sector at 250 MHz");
        assert_pattern(heap, seed, "the heap after the flash operations");
        assert_eq!(in_text(), 0x5053_524D);
    }

    // A change of the MPLL rate after `init` leaves the MPLL, and the clock tree's rate, as they
    // are.
    #[cfg(feature = "esp32s31")]
    #[test]
    fn test_mpll_change_after_init() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());

        ClockTree::with(|clocks| {
            esp_hal::clock::ll::configure_mpll_clk(clocks, MpllClkConfig::_500)
        });

        assert_eq!(
            ClockTree::with(|clocks| clocks.mpll_clk()),
            Some(MpllClkConfig::_400)
        );
        assert_eq!(mpll_clk_frequency(), 400_000_000);
        assert_eq!(mpll_in_hardware(), 400_000_000);
    }

    #[test]
    fn test_no_flash_page_mapped() {
        let _peripherals = esp_hal::init(esp_hal::Config::default());
        let spi0 = esp_hal::peripherals::SPI0::regs();

        for i in 0..1024 {
            spi0.mmu_item_index().write(|w| unsafe { w.bits(i) });
            assert!(
                !spi0.mmu_item_content().read().valid().bit_is_set(),
                "entry {} is mapped",
                i
            );
        }
    }

    // The start-up's MPLL rate stays when the configuration leaves the MPLL out. The clock tree
    // then holds the start-up's rate, which light sleep reads.
    #[cfg(feature = "esp32s31")]
    #[test]
    fn test_mpll_rate_without_mpll_config() {
        let mut clocks = ClockConfig::from(esp_hal::clock::CpuClock::default());
        clocks.mpll_clk = None;
        let _peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(clocks));

        assert_eq!(
            ClockTree::with(|clocks| clocks.mpll_clk()),
            Some(MpllClkConfig::_400)
        );
        assert_eq!(mpll_clk_frequency(), 400_000_000);
        assert_eq!(mpll_in_hardware(), 400_000_000);
    }

    // A request for the MPLL after `init` does not start the running MPLL again.
    #[cfg(feature = "esp32s31")]
    #[test]
    fn test_mpll_request_after_init() {
        let peripherals = esp_hal::init(esp_hal::Config::default());
        let psram = Psram::new(peripherals.PSRAM, PsramConfig::default());
        let (start, _) = psram.raw_parts();
        let heap = unsafe { core::slice::from_raw_parts_mut(start as *mut u32, UNCACHED_WORDS) };
        let seed = fresh_seed();
        fill_pattern(heap, seed);
        let before = mpll_clk_frequency();

        ClockTree::with(request_mpll_clk);

        assert_eq!(mpll_clk_frequency(), before);
        assert_eq!(mpll_in_hardware(), before);
        assert_eq!(in_text(), 0x5053_524D);
        assert_pattern(heap, seed, "the heap after the MPLL request");
    }

    // `Psram::new` with another timing leaves PSRAM and the MPLL as the start-up set them.
    #[cfg(feature = "esp32s31")]
    #[test]
    fn test_psram_new_with_another_frequency() {
        let peripherals = esp_hal::init(esp_hal::Config::default());
        let before = mpll_clk_frequency();
        let timing = PsramTimingParams::MHZ_250;

        let psram = Psram::new(
            peripherals.PSRAM,
            PsramConfig {
                timing,
                ..PsramConfig::default()
            },
        );

        let (start, size) = psram.raw_parts();
        assert!(size >= UNCACHED_WORDS * 4);
        let heap = unsafe { core::slice::from_raw_parts_mut(start as *mut u32, UNCACHED_WORDS) };
        let seed = fresh_seed();
        fill_pattern(heap, seed);
        assert_pattern(heap, seed, "the heap after Psram::new");
        assert_eq!(mpll_clk_frequency(), before);
        assert_eq!(mpll_in_hardware(), before);
        assert_eq!(in_text(), 0x5053_524D);
    }
}
