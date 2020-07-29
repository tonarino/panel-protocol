#![cfg_attr(not(feature = "std"), no_std)]

use arrayvec::{ArrayString, ArrayVec};

#[derive(Debug, PartialEq)]
pub enum Command {
    PowerCycler { slot: u8, state: bool },
    Brightness { value: u16 },
    Temperature { value: u16 },
}

pub enum Error {
    BufferFull,
    MalformedMessage,
}

// Rust doesn't support max() as a const fn, but this should be
// cmp::max(MAX_COMMAND_LEN, MAX_REPORT_LEN)
const MAX_SERIAL_MESSAGE_LEN: usize = 256;

const MAX_COMMAND_LEN: usize = 8;
const MAX_REPORT_LEN: usize = 256;
const MAX_DEBUG_MSG_LEN: usize = MAX_REPORT_LEN - 2;

impl Command {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Command, usize)>, ()> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'A', slot, state, ..] => Ok(Some((
                Command::PowerCycler {
                    slot,
                    state: state != 0,
                },
                3,
            ))),
            [b'B', msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Brightness { value }, 3)))
            }
            [b'C', msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Temperature { value }, 3)))
            }
            _ => Err(()),
        }
    }

    pub fn as_arrayvec(&self) -> ArrayVec<[u8; MAX_COMMAND_LEN]> {
        let mut buf = ArrayVec::new();
        match *self {
            Command::PowerCycler { slot, state } => {
                buf.push(b'A');
                buf.push(slot);
                buf.push(u8::from(state));
            }
            Command::Brightness { value } => {
                buf.push(b'B');
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            }
            Command::Temperature { value } => {
                buf.push(b'C');
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            }
        }
        buf
    }
}

#[derive(Debug, PartialEq)]
pub enum Report {
    DialValue {
        diff: i8,
    },
    Press,
    LongPress,
    EmergencyOff,
    Error {
        code: u16,
    },
    Debug {
        message: ArrayString<[u8; MAX_DEBUG_MSG_LEN]>,
    },
}

impl Report {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Report, usize)>, ()> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'V', diff, ..] => {
                let diff = i8::from_be_bytes([diff]);
                Ok(Some((Report::DialValue { diff }, 2)))
            }
            [b'P', ..] => Ok(Some((Report::Press, 1))),
            [b'L', ..] => Ok(Some((Report::LongPress, 1))),
            [b'X', ..] => Ok(Some((Report::EmergencyOff, 1))),
            [b'E', msb, lsb, ..] => {
                let code = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Report::Error { code }, 3)))
            }
            [b'D', len, ref message @ ..] if message.len() as u8 == len => Ok(Some((
                Report::Debug {
                    message: ArrayString::from(&core::str::from_utf8(message).unwrap()).unwrap(),
                },
                2 + message.len(),
            ))),
            _ => Err(()),
        }
    }

    pub fn as_arrayvec(&self) -> ArrayVec<[u8; MAX_REPORT_LEN]> {
        let mut buf = ArrayVec::new();
        match *self {
            Report::DialValue { diff } => {
                buf.push(b'V');
                buf.try_extend_from_slice(&diff.to_be_bytes()).unwrap();
            }
            Report::Press => {
                buf.push(b'P');
            }
            Report::LongPress => {
                buf.push(b'L');
            }
            Report::EmergencyOff => {
                buf.push(b'X');
            }
            Report::Error { code } => {
                buf.push(b'E');
                buf.try_extend_from_slice(&code.to_be_bytes()).unwrap();
            }
            Report::Debug { ref message } => {
                buf.push(b'D');
                buf.push(message.len() as u8);
                buf.try_extend_from_slice(message.as_bytes()).unwrap();
            }
        }
        buf
    }
}

pub struct Protocol {
    buf: arrayvec::ArrayVec<[u8; MAX_SERIAL_MESSAGE_LEN]>,
}

impl Protocol {
    pub fn new() -> Self {
        Self {
            buf: arrayvec::ArrayVec::new(),
        }
    }

    pub fn process_byte(&mut self, byte: u8) -> Result<Option<Command>, Error> {
        self.buf.try_push(byte).map_err(|_| Error::BufferFull)?;
        match Command::try_from(&self.buf[..]) {
            Ok(Some((command, bytes_read))) => {
                self.buf.drain(0..bytes_read);
                Ok(Some(command))
            }
            Err(_) => Err(Error::MalformedMessage),
            Ok(None) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrips_arrayvec() {
        let commands = [
            Command::PowerCycler {
                slot: 1,
                state: true,
            },
            Command::PowerCycler {
                slot: 20,
                state: false,
            },
            Command::Temperature { value: 100 },
            Command::Brightness { value: 100 },
        ];

        for command in commands.iter() {
            let (deserialized, _len) = Command::try_from(&command.as_arrayvec()[..])
                .unwrap()
                .unwrap();
            assert_eq!(command, &deserialized);
        }
    }

    #[test]
    fn report_roundtrips_arrayvec() {
        let reports = [
            Report::Press,
            Report::LongPress,
            Report::DialValue { diff: 100 },
            Report::EmergencyOff,
            Report::Error { code: 80 },
            Report::Debug {
                message: ArrayString::from("the frequency is 1000000000Hz").unwrap(),
            },
        ];

        for report in reports.iter() {
            let (deserialized, _len) = Report::try_from(&report.as_arrayvec()[..])
                .unwrap()
                .unwrap();
            assert_eq!(report, &deserialized);
        }
    }
}
