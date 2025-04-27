use core::num::NonZeroU16;
/// A cli tool to connect to a device that talks the protocol.
use failure::{err_msg, format_err, Error};
use panel_protocol::{ArrayVec, Command, PulseMode, Report, ReportReader, MAX_REPORT_LEN};
use serial_core::{BaudRate, SerialDevice, SerialPortSettings};
use serial_unix::TTYPort;
use std::{
    env, io,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

static TTY_TIMEOUT: Duration = Duration::from_millis(500);

struct Panel {
    tty: TTYPort,
    protocol: ReportReader,
    read_buf: [u8; MAX_REPORT_LEN],
}

impl Panel {
    fn new(tty_port: &str) -> Result<Self, Error> {
        let mut tty = TTYPort::open(&PathBuf::from(tty_port))?;
        tty.set_timeout(TTY_TIMEOUT)?;

        // The panel firmware runs at 115200 baud.
        // TODO: Remove this after switching to the native USB connection.
        let mut tty_settings = tty.read_settings()?;
        tty_settings.set_baud_rate(BaudRate::Baud115200)?;
        tty.write_settings(&tty_settings)?;

        let protocol = ReportReader::new();
        let read_buf = [0u8; MAX_REPORT_LEN];

        Ok(Self { tty, protocol, read_buf })
    }

    fn poll(&mut self) -> Result<ArrayVec<Report, MAX_REPORT_LEN>, Error> {
        match self.tty.read(&mut self.read_buf) {
            Ok(0) => Err(err_msg("End of file reached")),
            Ok(count) => self
                .protocol
                .process_bytes(&self.read_buf[..count])
                .map_err(|e| format_err!("Failed to process bytes: {:?}", e)),
            Err(e) if e.kind() != io::ErrorKind::TimedOut => Err(e.into()),
            Err(_) => Ok(ArrayVec::new()),
        }
    }

    fn send(&mut self, command: &Command) -> Result<(), Error> {
        self.tty.write_all(&command.as_arrayvec()[..])?;

        Ok(())
    }
}

fn print_usage(args: &[String]) {
    println!("Usage: {} <tty_port>", args[0]);
    println!();
    println!("The program initiates a serial connection with the device specified by the ");
    println!("tty_port, and prints every Report that comes in. You can also type or pipe ");
    println!("a Command in the RON format to send it to the device.");
    println!();
    println!("Example commands:");
    println!("  {}", ron::ser::to_string(&Command::Brightness { target: 0, value: 0 }).unwrap());
    println!(
        "  {}",
        ron::ser::to_string(&Command::Led {
            r: 255,
            g: 0,
            b: 0,
            pulse_mode: PulseMode::Breathing { interval_ms: NonZeroU16::new(4000u16).unwrap() }
        })
        .unwrap()
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        print_usage(&args);
        return;
    }

    let port = &args[1];
    let panel = match Panel::new(port) {
        Ok(panel) => Arc::new(Mutex::new(panel)),
        Err(e) => {
            println!("Failed to open TTY port {}: {}", port, e);
            return;
        },
    };

    let should_exit = Arc::new(AtomicBool::new(false));
    thread::spawn({
        let panel = panel.clone();
        let should_exit = should_exit.clone();
        move || loop {
            match panel.lock().unwrap().poll() {
                Ok(reports) => {
                    for report in reports {
                        println!("New serial message: {:?}", report);
                    }
                },
                Err(e) => {
                    println!("Failed to poll reports: {}", e);
                    should_exit.store(true, Ordering::SeqCst);
                    return;
                },
            }
            thread::sleep(Duration::from_millis(1));
        }
    });

    let stdin = io::stdin();
    while !should_exit.load(Ordering::SeqCst) {
        let mut line = String::new();
        if let Err(e) = stdin.read_line(&mut line) {
            panic!("Failed to read line: {}", e);
        }
        if line.is_empty() {
            // Exit when EOF is reached.
            break;
        }

        match ron::de::from_str(&line) {
            Ok(command) => match panel.lock().unwrap().send(&command) {
                Ok(_) => println!("Sent command: {:?}", &command),
                Err(e) => {
                    println!("Failed to send command {:?}: {}", &command, e);
                    return;
                },
            },
            Err(e) => {
                println!("Failed to parse \"{}\": {}", line.trim_end(), e);
            },
        }
    }
}
