// $ cargo rb ferris
#![no_std]
#![no_main]

use nrf_embassy as _; // global logger + panicking-behavior + memory layout

use embassy_executor::Spawner;
use embassy_nrf::gpio::Pin;
use embassy_nrf::{
    bind_interrupts,
    gpio::{Level, Output, OutputDrive},
    peripherals, spim,
};
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::{image::Image, pixelcolor::Rgb565, prelude::*};
use embedded_hal_bus::spi::ExclusiveDevice;
use tinybmp::Bmp;

use st7735_embassy::{self, buffer_size, ST7735};

bind_interrupts!(struct Irqs {
    SPIM3 => spim::InterruptHandler<peripherals::SPI3>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let mut config = spim::Config::default();
    config.frequency = spim::Frequency::M32;
    // spim args: spi instance, irq, sck, mosi/SDA, config
    let spim = spim::Spim::new_txonly(p.SPI3, Irqs, p.P1_05, p.P1_04, config);
    // cs_pin: chip select pin
    let cs_pin = Output::new(p.P1_03.degrade(), Level::Low, OutputDrive::Standard);
    let spi_dev = ExclusiveDevice::new(spim, cs_pin, Delay).unwrap();

    // rst:  display reset pin, managed at driver level
    let rst = Output::new(p.P1_01.degrade(), Level::High, OutputDrive::Standard);
    // dc: data/command selection pin, managed at driver level

    let dc = Output::new(p.P1_02.degrade(), Level::High, OutputDrive::Standard);

    let mut display = ST7735::<_, _, _, 160, 128, { buffer_size(160, 128) }>::new(
        spi_dev,
        dc,
        rst,
        Default::default(),
    );
    display.init(&mut Delay).await.unwrap();
    display.clear(Rgb565::BLACK).unwrap();

    let raw_image: Bmp<Rgb565> =
        Bmp::from_slice(include_bytes!("../../assets/ferris.bmp")).unwrap();
    let image = Image::new(&raw_image, Point::new(34, 24));

    image.draw(&mut display).unwrap();
    display.flush().await.unwrap();

    // LED is set to max, but can be modulated with pwm to change backlight brightness
    let mut backlight = Output::new(p.P0_03, Level::High, OutputDrive::Standard);
    loop {
        backlight.set_high();
        Timer::after(Duration::from_millis(700)).await;
        backlight.set_low();
        Timer::after(Duration::from_millis(300)).await;
    }
}
