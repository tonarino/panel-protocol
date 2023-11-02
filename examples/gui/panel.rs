use anyhow::{format_err, Error, Result};
use panel_protocol::{
    ArrayVec, Command, Report, ReportReader, MAX_REPORT_LEN, MAX_REPORT_QUEUE_LEN,
};
use serial_core::{BaudRate, SerialDevice, SerialPortSettings};
use serial_unix::TTYPort;
use std::{
    self, io,
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

static TTY_TIMEOUT: Duration = Duration::from_millis(500);

pub struct Panel {
    tty: TTYPort,
    protocol: ReportReader,
    read_buf: [u8; MAX_REPORT_LEN],
}

impl Panel {
    pub fn new(tty_port: &str) -> Result<Self, Error> {
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

    pub fn poll(&mut self) -> Result<ArrayVec<Report, MAX_REPORT_QUEUE_LEN>, Error> {
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

    pub fn send(&mut self, command: &Command) -> Result<(), Error> {
        self.tty.write_all(&command.as_arrayvec()[..])?;

        Ok(())
    }
}
