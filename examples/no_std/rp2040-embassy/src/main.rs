#![no_std]
#![no_main]

mod blink;
mod noline_async;

use crate::blink::blinking_led;
use defmt::{info, panic, unwrap};
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_usb::UsbDevice;
use embassy_usb::class::cdc_acm::{CdcAcmClass, ControlChanged, Receiver, Sender, State};
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use noline_async::cli;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    spawner.spawn(unwrap!(blinking_led(p.PIN_25.into())));

    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("Embassy");
        config.product = Some("USB-serial example");
        config.serial_number = Some("12345678");
        config.max_power = 100;
        config.max_packet_size_0 = 64;
        config
    };

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        let builder = embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [], // no msos descriptors
            CONTROL_BUF.init([0; 64]),
        );
        builder
    };

    // Create classes on the builder.
    let class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    type SenderType = Sender<'static, Driver<'static, USB>>;
    type ReceiverType = Receiver<'static, Driver<'static, USB>>;

    let (send, recv, ctrl) = class.split_with_control();
    static SEND: StaticCell<SenderType> = StaticCell::new();
    static RECV: StaticCell<ReceiverType> = StaticCell::new();
    static CONTROL: StaticCell<ControlChanged> = StaticCell::new();

    let sender = SEND.init(send);
    let recvr = RECV.init(recv);
    let control = CONTROL.init(ctrl);

    // Build the builder.
    let usb = builder.build();

    // Run the USB device.
    spawner.spawn(unwrap!(usb_task(usb)));

    // Run the CLI
    cli(sender, recvr, control).await;

    info!("Disconnected")
}

type MyUsbDriver = Driver<'static, USB>;
type MyUsbDevice = UsbDevice<'static, MyUsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}
