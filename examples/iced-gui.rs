use anyhow::{format_err, Error, Result};
use iced::{
    executor, slider, Align, Application, Checkbox, Column, Container, Element, Settings, Slider,
    Text,
};
use panel_protocol::{
    ArrayVec, Command, Report, ReportReader, MAX_REPORT_LEN, MAX_REPORT_QUEUE_LEN,
};
use serial_core::{BaudRate, SerialDevice, SerialPortSettings};
use serial_unix::TTYPort;
use std::{
    env, io,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
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

    fn poll(&mut self) -> Result<ArrayVec<[Report; MAX_REPORT_QUEUE_LEN]>, Error> {
        match self.tty.read(&mut self.read_buf) {
            Ok(0) => Err(format_err!("End of file reached")),
            Ok(count) => self
                .protocol
                .process_bytes(&self.read_buf[..count])
                .map_err(|e| format_err!("Failed to process bytes: {:?}", e)),
            Err(e) if e.kind() != io::ErrorKind::TimedOut => Err(e.into()),
            Err(_) => Ok(ArrayVec::new()),
        }
    }

    fn send(&mut self, command: &Command) -> Result<(), Error> {
        dbg!(command);
        self.tty.write_all(&command.as_arrayvec()[..])?;

        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct LedState {
    r: u8,
    g: u8,
    b: u8,
    pulse: bool,
}

impl From<LedState> for Command {
    fn from(state: LedState) -> Command {
        Command::Led { r: state.r, g: state.g, b: state.b, pulse: state.pulse }
    }
}

#[derive(Debug, Clone, Copy)]

enum Message {
    LedUpdate { r: Option<u8>, g: Option<u8>, b: Option<u8>, pulse: Option<bool> },
}
struct Configurator {
    report_rx: Receiver<Report>,
    command_tx: Sender<Command>,
    sliders: [slider::State; 3],
    led_state: LedState,
}

impl Application for Configurator {
    type Executor = executor::Default;
    type Flags = (Receiver<Report>, Sender<Command>);
    type Message = Message;

    fn new(flags: Self::Flags) -> (Self, iced::Command<Message>) {
        let (report_rx, command_tx) = flags;
        (
            Self {
                report_rx,
                command_tx,
                sliders: [slider::State::new(); 3],
                led_state: Default::default(),
            },
            async { Message::LedUpdate { r: None, g: None, b: None, pulse: None } }.into(),
        )
    }

    fn title(&self) -> String {
        "Panel Configurator - Iced".into()
    }

    fn update(
        &mut self,
        message: Self::Message,
        clipboard: &mut iced::Clipboard,
    ) -> iced::Command<Self::Message> {
        match message {
            Message::LedUpdate { r, g, b, pulse } => {
                if let Some(r) = r {
                    self.led_state.r = r;
                }
                if let Some(g) = g {
                    self.led_state.g = g;
                }
                if let Some(b) = b {
                    self.led_state.b = b;
                }
                if let Some(pulse) = pulse {
                    self.led_state.pulse = pulse;
                }
                self.command_tx.send(self.led_state.into()).unwrap();
            },
            _ => {},
        }
        iced::Command::none()
    }

    fn view(&mut self) -> Element<'_, Self::Message> {
        let [s1, s2, s3] = &mut self.sliders;
        let content = Column::new()
            .padding(50)
            .align_items(Align::Center)
            .push(Text::new("Panel Configurator\n").size(40))
            .push(Text::new(format!("LED Red: {}", self.led_state.r)))
            .push(
                Slider::new(s1, 0..=255, self.led_state.r, |v| Message::LedUpdate {
                    r: Some(v),
                    g: None,
                    b: None,
                    pulse: None,
                })
                .step(1),
            )
            .push(Text::new(format!("LED Green: {}", self.led_state.g)))
            .push(
                Slider::new(s2, 0..=255, self.led_state.g, |v| Message::LedUpdate {
                    r: None,
                    g: Some(v),
                    b: None,
                    pulse: None,
                })
                .step(1),
            )
            .push(Text::new(format!("LED Blue: {}", self.led_state.b)))
            .push(
                Slider::new(s3, 0..=255, self.led_state.b, |v| Message::LedUpdate {
                    r: None,
                    g: None,
                    b: Some(v),
                    pulse: None,
                })
                .step(1),
            )
            .push(Checkbox::new(self.led_state.pulse, "Pulse", |v| Message::LedUpdate {
                r: None,
                g: None,
                b: None,
                pulse: Some(v),
            }));
        Container::new(content).into()
    }
}

fn print_usage(args: &[String]) {
    println!("Usage: {} <tty_port>", args[0]);
    println!("");
    println!("The program initiates a serial connection with the device specified by the ");
    println!("tty_port, and prints every Report that comes in");
    println!("");
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        print_usage(&args);
        return Ok(());
    }

    let port = &args[1];
    let (report_tx, report_rx) = std::sync::mpsc::channel();
    let (command_tx, command_rx) = std::sync::mpsc::channel();

    let should_exit = Arc::new(AtomicBool::new(false));
    thread::spawn({
        let mut panel = Panel::new(port)?;
        let should_exit = should_exit.clone();
        move || loop {
            match panel.poll() {
                Ok(reports) => {
                    for report in reports {
                        println!("New serial message: {:?}", &report);
                        report_tx.send(report).unwrap();
                    }
                },
                Err(e) => {
                    eprintln!("Failed to poll reports: {}", e);
                    should_exit.store(true, Ordering::SeqCst);
                    return;
                },
            }

            while let Ok(command) = command_rx.try_recv() {
                panel.send(&command).unwrap();
            }
            thread::sleep(Duration::from_micros(50));
        }
    });

    Configurator::run(Settings::with_flags((report_rx, command_tx)))?;
    Ok(())
}
