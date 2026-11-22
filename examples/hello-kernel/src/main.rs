//! esp-sdk hello: boot an ESP32-S3 and run the TinyRTOS scheduler core.
//!
//! This is the first on-target exercise of the kernel: three priority classes,
//! a message queue between a fake sensor and a fake network task, and a
//! diagnostics dump every few seconds. It is a super-loop driving the
//! scheduler directly (the async executor lands with the SoC layer), but the
//! scheduling decisions, timers and IPC are the same code the executor will use.

#![no_std]
#![no_main]

use core::fmt::Write as _;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use esp_backtrace as _;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_hal::rtc_cntl::{Rtc, RwdtStage};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::time::Duration as HalDuration;
use esp_sdk_esp32s3::{I2c, Output, Serial, Timer};
use esp_sdk_hal::{GpioPin, Timer as _};
use esp_sdk_kernel::console::{Command, Console, ConsoleIo, LineEditor, LineEvent};
use esp_sdk_kernel::ipc::MessageQueue;
use esp_sdk_kernel::metrics::{Diagnostics, Health};
use esp_sdk_kernel::sched::{Class, Scheduler, TaskId};
use esp_sdk_kernel::supervisor::Supervisor;

esp_bootloader_esp_idf::esp_app_desc!();

static I2C_FOUND: AtomicU8 = AtomicU8::new(0);
static BOOT_US: AtomicU32 = AtomicU32::new(0);

const CTRL: TaskId = TaskId::new(0);
const SENSOR: TaskId = TaskId::new(1);
const NET: TaskId = TaskId::new(2);

fn now_us(start: Instant) -> u64 {
    start.elapsed().as_micros() as u64
}

fn cmd_version(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(out, "hello-kernel 0.1.0 (esp-sdk)");
}

fn cmd_sched(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(out, "classes=ctrl,app,net tasks=3");
}

fn cmd_status(_args: &[&str], out: &mut dyn ConsoleIo) {
    let uptime_ms = Instant::now().duration_since_epoch().as_micros() / 1_000;
    let _ = writeln!(
        out,
        "uptime_ms={uptime_ms} boot=1 health=ok reset={:?}",
        esp_hal::system::reset_reason()
    );
}

fn cmd_mem(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(
        out,
        "mem allocator=none kernel=static i2c_devices={}",
        I2C_FOUND.load(Ordering::Relaxed)
    );
}

fn cmd_selftest(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(
        out,
        "selftest: i2c_devices={} timer_boot_us={} rwdt=armed",
        I2C_FOUND.load(Ordering::Relaxed),
        BOOT_US.load(Ordering::Relaxed)
    );
}

fn cmd_reset(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(out, "reset: software reset requested");
    esp_hal::system::software_reset();
}

/// Stop feeding the hardware watchdog. The loop never returns, so the RWDT
/// (5 s stage 0) must reset the chip. Used by the starvation test.
fn cmd_starve(_args: &[&str], out: &mut dyn ConsoleIo) {
    let _ = writeln!(out, "RWDT: starvation test, feed stopped; expect reset in ~5 s");
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "irq-selftest")]
mod irq_selftest {
    use core::fmt::Write;
    use core::sync::atomic::{AtomicU32, Ordering};

    use esp_hal::handler;
    use esp_hal::time::Instant;
    use esp_sdk_esp32s3::{Timer, clear_timer_interrupt};
    use esp_sdk_hal::Timer as _;

    static COUNT: AtomicU32 = AtomicU32::new(0);
    static AT_US: AtomicU32 = AtomicU32::new(0);

    #[handler]
    fn on_timer() {
        clear_timer_interrupt();
        let now = Instant::now().duration_since_epoch().as_micros() as u32;
        AT_US.store(now, Ordering::Relaxed);
        COUNT.fetch_add(1, Ordering::Relaxed);
    }

    pub fn run(timer: &mut Timer<'_>, out: &mut impl Write) {
        Timer::bind_interrupt(on_timer);
        timer.listen();
        timer.arm(Instant::now().duration_since_epoch().as_micros() + 50_000);
        let mut seen = 0;
        for _ in 0..10 {
            let count = COUNT.load(Ordering::Relaxed);
            if count != seen {
                seen = count;
                let at = AT_US.load(Ordering::Relaxed);
                let now = Instant::now().duration_since_epoch().as_micros() as u32;
                let _ = writeln!(
                    out,
                    "irq: n={count} isr_at={at}us main_saw={now}us delta={}us",
                    now.wrapping_sub(at)
                );
                timer.clear_interrupt();
                timer.arm(Instant::now().duration_since_epoch().as_micros() + 100_000);
            }
            let spin = Instant::now().duration_since_epoch().as_micros();
            while Instant::now().duration_since_epoch().as_micros() < spin + 50_000 {}
        }
        timer.disarm();
        let _ = writeln!(out, "irq selftest: total={}", COUNT.load(Ordering::Relaxed));
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut serial = Serial::uart0(peripherals.UART0, peripherals.GPIO43, peripherals.GPIO44).unwrap();
    let mut led = Output::new(peripherals.GPIO2, false);

    let _ = writeln!(serial);
    let _ = writeln!(serial, "Sprchuoi ESP32 Boot");
    let _ = writeln!(serial, "==============================================");
    let _ = writeln!(serial, " esp-sdk hello-kernel on ESP32-S3");
    let _ = writeln!(serial, " kernel: scheduler + IPC + diagnostics demo");
    let _ = writeln!(serial, "==============================================");

    // I2C bus scan through the SDK HAL. On a bare devkit nothing answers; the
    // point is that the bus is driven by our trait, not by esp-hal directly.
    if let Ok(mut i2c) = I2c::new(peripherals.I2C0, peripherals.GPIO8, peripherals.GPIO9) {
        let mut found = 0u8;
        for address in 0x08..=0x77 {
            if i2c.probe(address) {
                let _ = writeln!(serial, "i2c: device at 0x{address:02x}");
                found += 1;
            }
        }
        I2C_FOUND.store(found, Ordering::Relaxed);
        let _ = writeln!(serial, "i2c scan: {found} device(s)");
    }

    // Tickless alarm primitive: arm a 50 ms one-shot through the HAL, then
    // disarm it. The super-loop does not need it yet, the executor will.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut timer = Timer::new(timg0.timer0);
    let boot_us = timer.now_us();
    BOOT_US.store(boot_us as u32, Ordering::Relaxed);
    timer.arm(boot_us.saturating_add(50_000));
    timer.disarm();
    let _ = writeln!(serial, "timer: boot_us={boot_us}");

    #[cfg(feature = "irq-selftest")]
    irq_selftest::run(&mut timer, &mut serial);

    // Hardware watchdog: the supervisor's deadline is software, the RWDT is the
    // thing that actually resets a hung chip.
    let mut rtc = Rtc::new(peripherals.RTC_TIMER);
    rtc.rwdt
        .set_timeout(RwdtStage::Stage0, HalDuration::from_millis(5_000));
    rtc.rwdt.enable();

    // The console framework running over the SDK HAL: UART0 is a `fmt::Write`
    // sink, commands are static, no allocation.
    let mut console = Console::<7>::new();
    console
        .register(Command::new("version", "print firmware version", cmd_version))
        .unwrap();
    console
        .register(Command::new("sched", "show scheduler summary", cmd_sched))
        .unwrap();
    console
        .register(Command::new("rwdt-starve", "stop the RWDT feed (expect reset)", cmd_starve))
        .unwrap();
    console
        .register(Command::new("status", "uptime, boot and reset reason", cmd_status))
        .unwrap();
    console
        .register(Command::new("mem", "memory layout summary", cmd_mem))
        .unwrap();
    console
        .register(Command::new("selftest", "i2c/timer/watchdog summary", cmd_selftest))
        .unwrap();
    console
        .register(Command::new("reset", "software reset", cmd_reset))
        .unwrap();
    let _ = console.execute("help", &mut serial);

    let mut supervisor = Supervisor::<4>::new(5_000_000, 3);
    supervisor.register(CTRL.raw(), 500_000, 0).unwrap();

    let mut editor = LineEditor::<64>::new();

    let start = Instant::now();

    let mut scheduler = Scheduler::new();
    scheduler.register(CTRL, Class::Ctrl).unwrap();
    scheduler.register(SENSOR, Class::App).unwrap();
    scheduler.register(NET, Class::Net).unwrap();

    let mut readings: MessageQueue<u32, 8> = MessageQueue::new();
    let mut diagnostics = Diagnostics::<4, 2, 4>::new();
    let packets = diagnostics.metrics.define("mqtt.packets").unwrap();
    let reads = diagnostics.metrics.define("sensor.reads").unwrap();
    let heap = diagnostics.memory.register("heap", 64 * 1024).unwrap();
    diagnostics.memory.set_used(heap, 12 * 1024);

    // First deadlines: control every 250 ms, sensor every 500 ms, net every 1 s.
    scheduler.sleep_until(CTRL, 0).unwrap();
    scheduler.sleep_until(SENSOR, 0).unwrap();
    scheduler.sleep_until(NET, 0).unwrap();

    let mut last_dump_us = 0u64;

    loop {
        let now = now_us(start);
        scheduler.advance_to(now);
        supervisor.feed(now);
        rtc.rwdt.feed();

        // Interactive console over the SDK HAL: the kernel's LineEditor handles
        // printable bytes, backspace and Ctrl-C; we only echo and dispatch.
        while let Some(byte) = serial.read_byte() {
            match editor.feed(byte) {
                LineEvent::Pending => {
                    if byte == 0x08 || byte == 0x7f {
                        let _ = write!(serial, "\u{8} \u{8}");
                    } else if (0x20..=0x7e).contains(&byte) {
                        let _ = write!(serial, "{}", byte as char);
                    }
                }
                LineEvent::Submitted => {
                    let _ = writeln!(serial);
                    let mut buffer = [0u8; 64];
                    let length = editor.copy_line(&mut buffer);
                    if let Ok(text) = core::str::from_utf8(&buffer[..length]) {
                        let _ = console.execute(text, &mut serial);
                    }
                    editor.clear();
                }
                LineEvent::Cancelled => {
                    let _ = writeln!(serial, "^C");
                    editor.clear();
                }
                LineEvent::Rejected => {}
            }
        }

        while let Some(task) = scheduler.next_ready() {
            match task {
                CTRL => {
                    let _ = writeln!(serial, "[ctrl] tick t={} ms", now / 1000);
                    supervisor.beat(CTRL.raw(), now);
                    led.set(!led.get());
                    scheduler.sleep_for(CTRL, 250_000).unwrap();
                }
                SENSOR => {
                    // Fake a sensor reading and hand it to the network task.
                    let reading = (now / 1000) as u32 % 1000;
                    if readings.push(reading).is_err() {
                        diagnostics.faults.record(0x0101, now, reading);
                    }
                    diagnostics.metrics.incr(reads);
                    scheduler.sleep_for(SENSOR, 500_000).unwrap();
                }
                NET => {
                    while let Some(reading) = readings.pop() {
                        let _ = writeln!(
                            serial,
                            "[net ] publish sensor={} t={} ms",
                            reading,
                            now / 1000
                        );
                        diagnostics.metrics.incr(packets);
                    }
                    scheduler.sleep_for(NET, 1_000_000).unwrap();
                }
                _ => {}
            }
        }

        if let Some(event) = supervisor.poll(now) {
            let _ = writeln!(serial, "supervisor: {event:?}");
        }

        if now - last_dump_us >= 5_000_000 {
            last_dump_us = now;
            diagnostics.uptime_us = now;
            diagnostics.boot_count = 1;

            let mut dump = DumpLine::default();
            diagnostics.dump(&mut dump).unwrap();
            let _ = writeln!(serial, "{}", dump.as_str());

            if diagnostics.health() == Health::Critical {
                diagnostics.faults.record(0x0202, now, 0);
            }
        }
    }
}

/// Small fixed buffer so the dump can be printed without allocating.
struct DumpLine {
    buffer: [u8; 768],
    len: usize,
}

impl Default for DumpLine {
    fn default() -> Self {
        Self {
            buffer: [0; 768],
            len: 0,
        }
    }
}

impl DumpLine {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("<dump>")
    }
}

impl core::fmt::Write for DumpLine {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let bytes = text.as_bytes();
        if self.len + bytes.len() > self.buffer.len() {
            return Err(core::fmt::Error);
        }
        self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}
