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

use esp_backtrace as _;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_sdk_esp32s3::Serial;
use esp_sdk_kernel::console::{Command, Console, ConsoleIo};
use esp_sdk_kernel::ipc::MessageQueue;
use esp_sdk_kernel::metrics::{Diagnostics, Health};
use esp_sdk_kernel::sched::{Class, Scheduler, TaskId};
use esp_sdk_kernel::supervisor::Supervisor;

esp_bootloader_esp_idf::esp_app_desc!();

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

#[main]
fn main() -> ! {
    let _peripherals = esp_hal::init(esp_hal::Config::default());
    let mut serial = Serial::uart0();

    let _ = writeln!(serial);
    let _ = writeln!(serial, "Sprchuoi ESP32 Boot");
    let _ = writeln!(serial, "==============================================");
    let _ = writeln!(serial, " esp-sdk hello-kernel on ESP32-S3");
    let _ = writeln!(serial, " kernel: scheduler + IPC + diagnostics demo");
    let _ = writeln!(serial, "==============================================");

    // The console framework running over the SDK HAL: UART0 is a `fmt::Write`
    // sink, commands are static, no allocation.
    let mut console = Console::<2>::new();
    console
        .register(Command::new("version", "print firmware version", cmd_version))
        .unwrap();
    console
        .register(Command::new("sched", "show scheduler summary", cmd_sched))
        .unwrap();
    let _ = console.execute("help", &mut serial);

    let mut supervisor = Supervisor::<4>::new(5_000_000, 3);
    supervisor.register(CTRL.raw(), 500_000, 0).unwrap();

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

        while let Some(task) = scheduler.next_ready() {
            match task {
                CTRL => {
                    let _ = writeln!(serial, "[ctrl] tick t={} ms", now / 1000);
                    supervisor.beat(CTRL.raw(), now);
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
