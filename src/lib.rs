#![no_std]
pub mod instruction;
use crate::instruction::Instruction;
use core::convert::Infallible;
use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::spi::SpiDevice;

/// Calculates the required buffer size.
/// Inspired by `embedded-graphics`-`FrameBuffer` <https://docs.rs/embedded-graphics/latest/embedded_graphics/framebuffer/struct.Framebuffer.html>
/// Once `feature(generic_const_exprs)` <https://github.com/rust-lang/rust/issues/76560> this is not needed anymore.
#[must_use]
pub const fn buffer_size(width: u16, height: u16) -> usize {
    width as usize * height as usize * 2
}

/// Display Pixel Color Mode
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum PixelColor {
    /// Red, Green, Blue,
    RGB = 0x00,
    /// Blue, Green, Red,
    BGR = 0x08,
}

/// Async ST7735 LCD display driver.
pub struct ST7735IF<SPI, DC, RST>
where
    SPI: SpiDevice,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    /// SPI
    spi: SPI,
    /// Data/command pin.
    dc: DC,
    /// Reset pin.
    rst: RST,
    /// Whether the display is RGB or BGR
    rgb: PixelColor,
    /// Whether the colours are inverted (true) or not (false)
    inverted: bool,
    /// Global image offset
    dx: u16,
    dy: u16,
    orientation: Orientation,
}
pub struct ST7735<SPI, DC, RST, const WIDTH: u16, const HEIGHT: u16, const N: usize>
where
    SPI: SpiDevice,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    iface: ST7735IF<SPI, DC, RST>,
    buffer: [u8; N],
}

/// Display orientation.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Orientation {
    Portrait = 0x00,
    Landscape = 0x60,
    PortraitSwapped = 0xC0,
    LandscapeSwapped = 0xA0,
}

/// Display Settings
pub struct Config {
    /// `PixelColor`
    pub rgb: PixelColor,
    /// Colors inverted.
    pub inverted: bool,
    /// Display orientation
    pub orientation: Orientation,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rgb: PixelColor::RGB,
            inverted: false,
            orientation: Orientation::Landscape,
        }
    }
}

struct Command<'a> {
    instruction: Instruction,
    params: &'a [u8],
    delay_time: u32,
}

impl<'a> Command<'a> {
    fn new(instruction: Instruction, params: &'a [u8], delay_time: u32) -> Self {
        Self {
            instruction,
            params,
            delay_time,
        }
    }
}

impl<SPI, DC, RST, E> ST7735IF<SPI, DC, RST>
where
    SPI: SpiDevice<Error = E>,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    /// Creates a new driver instance that uses hardware SPI.
    pub fn new(spi: SPI, dc: DC, rst: RST, config: Config) -> Self {
        Self {
            spi,
            dc,
            rst,
            rgb: config.rgb,
            inverted: config.inverted,
            orientation: config.orientation,
            dx: 0,
            dy: 0,
        }
    }

    /// Runs commands to initialize the display.
    pub async fn init<D>(&mut self, delay: &mut D) -> Result<(), Error<E>>
    where
        D: DelayNs,
    {
        self.hard_reset(delay).await?;
        let dc = &mut self.dc;
        let inverted = self.inverted;
        let rgb = &[self.rgb as u8];

        let commands = [
            Command::new(Instruction::SWRESET, &[], 200),
            Command::new(Instruction::SLPOUT, &[], 200),
            Command::new(Instruction::FRMCTR1, &[0x01, 0x2C, 0x2D], 0),
            Command::new(Instruction::FRMCTR2, &[0x01, 0x2C, 0x2D], 0),
            Command::new(
                Instruction::FRMCTR3,
                &[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D],
                0,
            ),
            Command::new(Instruction::INVCTR, &[0x07], 0),
            Command::new(Instruction::PWCTR1, &[0xA2, 0x02, 0x84], 0),
            Command::new(Instruction::PWCTR2, &[0xC5], 0),
            Command::new(Instruction::PWCTR3, &[0x0A, 0x00], 0),
            Command::new(Instruction::PWCTR4, &[0x8A, 0x2A], 0),
            Command::new(Instruction::PWCTR5, &[0x8A, 0xEE], 0),
            Command::new(Instruction::VMCTR1, &[0x0E], 0),
            Command::new(
                if inverted {
                    Instruction::INVON
                } else {
                    Instruction::INVOFF
                },
                &[],
                0,
            ),
            Command::new(Instruction::MADCTL, rgb, 0),
            Command::new(Instruction::COLMOD, &[0x05], 0),
            Command::new(Instruction::DISPON, &[], 200),
        ];

        for Command {
            instruction,
            params,
            delay_time,
        } in commands
        {
            dc.set_low().ok();
            let mut data = [0_u8; 1];
            data.copy_from_slice(&[instruction as u8]);
            self.spi.write(&data).await.map_err(Error::Comm)?;
            if !params.is_empty() {
                dc.set_high().ok();
                let mut buf = [0_u8; 8];
                buf[..params.len()].copy_from_slice(params);
                self.spi
                    .write(&buf[..params.len()])
                    .await
                    .map_err(Error::Comm)?;
            }
            if delay_time > 0 {
                delay.delay_ms(delay_time).await;
            }
        }

        self.set_orientation(self.orientation).await?;
        Ok(())
    }

    pub async fn hard_reset<D>(&mut self, delay: &mut D) -> Result<(), Error<E>>
    where
        D: DelayNs,
    {
        self.rst.set_high().map_err(Error::Pin)?;
        delay.delay_ms(10).await;
        self.rst.set_low().map_err(Error::Pin)?;
        delay.delay_ms(10).await;
        self.rst.set_high().map_err(Error::Pin)
    }

    pub async fn set_orientation(&mut self, orientation: Orientation) -> Result<(), Error<E>> {
        self.write_command(Instruction::MADCTL, &[orientation as u8 | self.rgb as u8])
            .await?;

        self.orientation = orientation;
        Ok(())
    }

    async fn write_command(
        &mut self,
        instruction: Instruction,
        params: &[u8],
    ) -> Result<(), Error<E>> {
        let dc = &mut self.dc;
        dc.set_low().ok();
        let mut data = [0_u8; 1];
        data.copy_from_slice(&[instruction as u8]);
        self.spi.write(&data).await.map_err(Error::Comm)?;
        if !params.is_empty() {
            dc.set_high().ok();
            let mut buf = [0_u8; 8];
            buf[..params.len()].copy_from_slice(params);
            self.spi
                .write(&buf[..params.len()])
                .await
                .map_err(Error::Comm)?;
        }
        Ok(())
    }

    fn start_data(&mut self) -> Result<(), Error<E>> {
        self.dc.set_high().map_err(Error::Pin)
    }

    async fn write_data(&mut self, data: &[u8]) -> Result<(), Error<E>> {
        let mut buf = [0_u8; 8];
        buf[..data.len()].copy_from_slice(data);
        self.spi
            .write(&buf[..data.len()])
            .await
            .map_err(Error::Comm)
    }

    /// Sets the global offset of the displayed image
    pub fn set_offset(&mut self, dx: u16, dy: u16) {
        self.dx = dx;
        self.dy = dy;
    }

    /// Sets the address window for the display.
    pub async fn set_address_window(
        &mut self,
        sx: u16,
        sy: u16,
        ex: u16,
        ey: u16,
    ) -> Result<(), Error<E>> {
        self.write_command(Instruction::CASET, &[]).await?;
        self.start_data()?;
        let sx_bytes = (sx + self.dx).to_be_bytes();
        let ex_bytes = (ex + self.dx).to_be_bytes();
        self.write_data(&[sx_bytes[0], sx_bytes[1], ex_bytes[0], ex_bytes[1]])
            .await?;
        self.write_command(Instruction::RASET, &[]).await?;
        self.start_data()?;
        let sy_bytes = (sy + self.dy).to_be_bytes();
        let ey_bytes = (ey + self.dy).to_be_bytes();
        self.write_data(&[sy_bytes[0], sy_bytes[1], ey_bytes[0], ey_bytes[1]])
            .await
    }

    pub async fn flush_frame<const N: usize>(&mut self, frame: &Frame<N>) -> Result<(), Error<E>> {
        self.set_address_window(0, 0, frame.width as u16 - 1, frame.height as u16 - 1)
            .await?;
        self.write_command(Instruction::RAMWR, &[]).await?;
        self.start_data()?;
        self.spi.write(&frame.buffer).await.map_err(Error::Comm)
    }
}

impl<SPI, DC, RST, E, const WIDTH: u16, const HEIGHT: u16, const N: usize>
    ST7735<SPI, DC, RST, WIDTH, HEIGHT, N>
where
    SPI: SpiDevice<Error = E>,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    #[allow(dead_code)]
    const BUFFER_SIZE: usize = buffer_size(WIDTH, HEIGHT);

    #[allow(dead_code)]
    /// Static assertion that N is correct.
    // MSRV: remove N when constant generic expressions are stabilized
    // See <https://github.com/rust-lang/rust/issues/76560>
    const CHECK_N: () = assert!(
        N == Self::BUFFER_SIZE,
        "Invalid N: see N must be equal to WIDTH x HEIGHT x 2!"
    );

    /// Creates a new driver instance that uses hardware SPI.
    pub fn new(spi: SPI, dc: DC, rst: RST, config: Config) -> Self {
        Self {
            iface: ST7735IF::new(spi, dc, rst, config),
            buffer: [0; N],
        }
    }

    /// Runs commands to initialize the display.
    pub async fn init<D>(&mut self, delay: &mut D) -> Result<(), Error<E>>
    where
        D: DelayNs,
    {
        self.iface.init(delay).await?;

        Ok(())
    }

    /// Transfer the internal buffer to the LCD display.
    pub async fn flush(&mut self) -> Result<(), Error<E>> {
        self.iface
            .set_address_window(0, 0, WIDTH - 1, HEIGHT - 1)
            .await?;
        self.iface.write_command(Instruction::RAMWR, &[]).await?;
        self.iface.start_data()?;
        let buf = &self.buffer;
        self.iface.spi.write(buf).await.map_err(Error::Comm)
    }

    /// Transfer the external buffer to the LCD display.
    pub async fn flush_buffer(&mut self, buf: &[u8]) -> Result<(), Error<E>> {
        self.iface
            .set_address_window(0, 0, WIDTH - 1, HEIGHT - 1)
            .await?;
        self.iface.write_command(Instruction::RAMWR, &[]).await?;
        self.iface.start_data()?;
        self.iface.spi.write(buf).await.map_err(Error::Comm)
    }

    /// Sets a pixel color at the given coords.
    pub fn set_pixel(&mut self, x: u16, y: u16, color: u16) {
        let idx = match self.iface.orientation {
            Orientation::Landscape | Orientation::LandscapeSwapped => {
                if x >= WIDTH {
                    return;
                }
                (usize::from(y) * usize::from(WIDTH)) + usize::from(x)
            }

            Orientation::Portrait | Orientation::PortraitSwapped => {
                if y >= WIDTH {
                    return;
                }
                (usize::from(y) * usize::from(HEIGHT)) + usize::from(x)
            }
        } * 2;

        // Split 16 bit value into two bytes
        let low = (color & 0xff) as u8;
        let high = ((color & 0xff00) >> 8) as u8;
        if idx >= self.buffer.len() - 1 {
            return;
        }
        self.buffer[idx] = high;
        self.buffer[idx + 1] = low;
    }

    /// Sets the global offset of the displayed image
    pub fn set_offset(&mut self, dx: u16, dy: u16) {
        self.iface.set_offset(dx, dy);
    }
}

extern crate embedded_graphics_core;
use self::embedded_graphics_core::{
    draw_target::DrawTarget,
    pixelcolor::{
        raw::{RawData, RawU16},
        Rgb565,
    },
    prelude::*,
};

impl<SPI, DC, RST, E, const WIDTH: u16, const HEIGHT: u16, const N: usize> DrawTarget
    for ST7735<SPI, DC, RST, WIDTH, HEIGHT, N>
where
    SPI: SpiDevice<Error = E>,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    type Error = ();
    type Color = Rgb565;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let bb = self.bounding_box();

        pixels
            .into_iter()
            .filter(|Pixel(pos, _color)| bb.contains(*pos))
            .for_each(|Pixel(pos, color)| {
                self.set_pixel(pos.x as u16, pos.y as u16, RawU16::from(color).into_inner());
            });

        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let c = RawU16::from(color).into_inner();
        for i in 0..N {
            self.buffer[i] = if i % 2 == 0 {
                ((c & 0xff00) >> 8) as u8
            } else {
                (c & 0xff) as u8
            };
        }
        Ok(())
    }
}

impl<SPI, DC, RST, E, const WIDTH: u16, const HEIGHT: u16, const N: usize> OriginDimensions
    for ST7735<SPI, DC, RST, WIDTH, HEIGHT, N>
where
    SPI: SpiDevice<Error = E>,
    DC: OutputPin<Error = Infallible>,
    RST: OutputPin<Error = Infallible>,
{
    fn size(&self) -> Size {
        Size::new(u32::from(WIDTH), u32::from(HEIGHT))
    }
}

#[derive(Debug)]
pub enum Error<E = ()> {
    /// Communication error
    Comm(E),
    /// Pin setting error
    Pin(Infallible),
}

pub struct Frame<const N: usize> {
    pub width: u32,
    pub height: u32,
    pub orientation: Orientation,
    pub buffer: [u8; N],
}

impl<const N: usize> Frame<N> {
    #[must_use]
    pub fn new(width: u32, height: u32, orientation: Orientation, buffer: [u8; N]) -> Self {
        Self {
            width,
            height,
            orientation,
            buffer,
        }
    }
    pub fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565) {
        let color = RawU16::from(color).into_inner();
        let idx = match self.orientation {
            Orientation::Landscape | Orientation::LandscapeSwapped => {
                if u32::from(x) >= self.width {
                    return;
                }
                (usize::from(y) * (self.width as usize)) + usize::from(x)
            }

            Orientation::Portrait | Orientation::PortraitSwapped => {
                if u32::from(y) >= self.height {
                    return;
                }
                ((y as usize) * self.height as usize) + (x as usize)
            }
        } * 2;

        // Split 16 bit value into two bytes
        let low = (color & 0xff) as u8;
        let high = ((color & 0xff00) >> 8) as u8;
        if idx >= self.buffer.len() - 1 {
            return;
        }
        self.buffer[idx] = high;
        self.buffer[idx + 1] = low;
    }
}
impl<const N: usize> Default for Frame<N> {
    fn default() -> Self {
        Self {
            width: 160,
            height: 128,
            orientation: Orientation::Landscape,
            buffer: [0; N],
        }
    }
}

impl<const N: usize> DrawTarget for Frame<N> {
    type Error = ();
    type Color = Rgb565;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let bb = self.bounding_box();
        pixels
            .into_iter()
            .filter(|Pixel(pos, _color)| bb.contains(*pos))
            .for_each(|Pixel(pos, color)| self.set_pixel(pos.x as u16, pos.y as u16, color));
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let c = RawU16::from(color).into_inner();
        for i in 0..N {
            self.buffer[i] = if i % 2 == 0 {
                ((c & 0xff00) >> 8) as u8
            } else {
                (c & 0xff) as u8
            };
        }
        Ok(())
    }
}

impl<const N: usize> OriginDimensions for Frame<N> {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}
