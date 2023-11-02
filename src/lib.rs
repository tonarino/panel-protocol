#![cfg_attr(not(feature = "std"), no_std)]

use core::{
    convert::{TryFrom, TryInto},
    num::NonZeroU16,
};

pub use arrayvec::{ArrayString, ArrayVec};

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
pub enum Command {
    PowerCycler { slot: u8, state: bool },
    Brightness { target: u8, value: u16 },
    Temperature { target: u8, value: u16 },
    Led { r: u8, g: u8, b: u8, pulse_mode: PulseMode },
    FanSpeed { target: u8, value: u16 },
    Bootload, // Restart in bootloader mode.
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
pub enum PulseMode {
    Solid,
    Breathing { interval_ms: NonZeroU16 },
    DialTurn,
}

impl From<PulseMode> for [u8; 3] {
    fn from(pulse_mode: PulseMode) -> Self {
        match pulse_mode {
            PulseMode::Solid => [b'S', 0, 0],
            PulseMode::DialTurn => [b'D', 0, 0],
            PulseMode::Breathing { interval_ms } => {
                let interval_bytes = u16::from(interval_ms).to_be_bytes();
                [b'B', interval_bytes[0], interval_bytes[1]]
            },
        }
    }
}

impl TryFrom<[u8; 3]> for PulseMode {
    type Error = Error;

    fn try_from(bytes: [u8; 3]) -> Result<Self, Error> {
        match bytes {
            [b'S', ..] => Ok(PulseMode::Solid),
            [b'D', ..] => Ok(PulseMode::DialTurn),
            [b'B', msb, lsb] => {
                let interval_value = u16::from_be_bytes([msb, lsb]);
                NonZeroU16::new(interval_value).map_or_else(
                    || Err(Error::MalformedMessage),
                    |interval_ms| Ok(PulseMode::Breathing { interval_ms }),
                )
            },
            _ => Err(Error::MalformedMessage),
        }
    }
}
#[derive(Debug)]
pub enum Error {
    BufferFull,
    MalformedMessage,
    CommandQueueFull,
    ReportQueueFull,
}

#[cfg(feature = "std")]
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

// Rust doesn't support max() as a const fn, but this should be
// cmp::max(MAX_COMMAND_LEN, MAX_REPORT_LEN)
pub const MAX_SERIAL_MESSAGE_LEN: usize = 256;

pub const MAX_COMMAND_LEN: usize = 8;
pub const MAX_REPORT_LEN: usize = 256;
pub const MAX_DEBUG_MSG_LEN: usize = MAX_REPORT_LEN - 2;

impl Command {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Command, usize)>, Error> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'A', slot, state, ..] => {
                Ok(Some((Command::PowerCycler { slot, state: state != 0 }, 3)))
            },
            [b'B', target, msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Brightness { target, value }, 4)))
            },
            [b'C', target, msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Temperature { target, value }, 4)))
            },
            [b'D', r, g, b, pulse_mode, pmsb, plsb, ..] => Ok(Some((
                Command::Led { r, g, b, pulse_mode: [pulse_mode, pmsb, plsb].try_into()? },
                7,
            ))),
            [b'E', ..] => Ok(Some((Command::Bootload, 1))),
            [b'F', target, msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::FanSpeed { target, value }, 4)))
            },
            [header, ..] if b"ABCD".contains(&header) => Ok(None),
            _ => Err(Error::MalformedMessage),
        }
    }

    pub fn as_arrayvec(&self) -> ArrayVec<u8, MAX_COMMAND_LEN> {
        let mut buf = ArrayVec::new();

        match *self {
            Command::PowerCycler { slot, state } => {
                buf.push(b'A');
                buf.push(slot);
                buf.push(u8::from(state));
            },
            Command::Brightness { target, value } => {
                buf.push(b'B');
                buf.push(target);
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            },
            Command::Temperature { target, value } => {
                buf.push(b'C');
                buf.push(target);
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            },
            Command::Led { r, g, b, pulse_mode } => {
                buf.push(b'D');
                buf.push(r);
                buf.push(g);
                buf.push(b);
                let pulse_mode_bytes: [u8; 3] = pulse_mode.into();
                buf.try_extend_from_slice(&pulse_mode_bytes).unwrap();
            },
            Command::Bootload => buf.push(b'E'),
            Command::FanSpeed { target, value } => {
                buf.push(b'F');
                buf.push(target);
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            },
        }
        buf
    }
}

type DebugMessage = ArrayString<MAX_DEBUG_MSG_LEN>;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
pub enum Report {
    Heartbeat,
    DialValue {
        diff: i8,
    },
    Press,
    Release,
    EmergencyOff,
    Error {
        code: u16,
    },
    Debug {
        #[cfg_attr(
            feature = "serde_support",
            serde(
                serialize_with = "serialize_debug_message",
                deserialize_with = "deserialize_debug_message"
            )
        )]
        message: DebugMessage,
    },
}

impl Report {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Report, usize)>, Error> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'H', ..] => Ok(Some((Report::Heartbeat, 1))),
            [b'V', diff, ..] => {
                let diff = i8::from_be_bytes([diff]);
                Ok(Some((Report::DialValue { diff }, 2)))
            },
            [b'P', ..] => Ok(Some((Report::Press, 1))),
            [b'R', ..] => Ok(Some((Report::Release, 1))),
            [b'X', ..] => Ok(Some((Report::EmergencyOff, 1))),
            [b'E', msb, lsb, ..] => {
                let code = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Report::Error { code }, 3)))
            },
            [b'D', len, ref message @ ..] if message.len() as u8 == len => Ok(Some((
                Report::Debug {
                    message: ArrayString::from(core::str::from_utf8(message).unwrap()).unwrap(),
                },
                2 + message.len(),
            ))),
            [header, ..] if b"VED".contains(&header) => Ok(None),
            _ => Err(Error::MalformedMessage),
        }
    }

    pub fn as_arrayvec(&self) -> ArrayVec<u8, MAX_REPORT_LEN> {
        let mut buf = ArrayVec::new();

        match *self {
            Report::Heartbeat => {
                buf.push(b'H');
            },
            Report::DialValue { diff } => {
                buf.push(b'V');
                buf.try_extend_from_slice(&diff.to_be_bytes()).unwrap();
            },
            Report::Press => {
                buf.push(b'P');
            },
            Report::Release => {
                buf.push(b'R');
            },
            Report::EmergencyOff => {
                buf.push(b'X');
            },
            Report::Error { code } => {
                buf.push(b'E');
                buf.try_extend_from_slice(&code.to_be_bytes()).unwrap();
            },
            Report::Debug { ref message } => {
                buf.push(b'D');
                buf.push(message.len() as u8);
                buf.try_extend_from_slice(message.as_bytes()).unwrap();
            },
        }
        buf
    }
}

#[cfg(feature = "serde_support")]
fn serialize_debug_message<S>(value: &DebugMessage, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value.as_str())
}

#[cfg(feature = "serde_support")]
fn deserialize_debug_message<'de, D>(deserializer: D) -> Result<DebugMessage, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct DebugMessageVisitor(std::marker::PhantomData<DebugMessage>);

    impl<'de> serde::de::Visitor<'de> for DebugMessageVisitor {
        type Value = DebugMessage;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            DebugMessage::from(value).map_err(serde::de::Error::custom)
        }
    }

    deserializer.deserialize_any(DebugMessageVisitor(std::marker::PhantomData))
}

pub struct ReportReader {
    pub buf: ArrayVec<u8, MAX_SERIAL_MESSAGE_LEN>,
}

impl ReportReader {
    pub fn new() -> Self {
        Self { buf: ArrayVec::new() }
    }

    pub fn process_bytes<const MAX_REPORT_QUEUE_LEN: usize>(
        &mut self,
        bytes: &[u8],
    ) -> Result<ArrayVec<Report, MAX_REPORT_QUEUE_LEN>, Error> {
        self.buf.try_extend_from_slice(bytes).map_err(|_| Error::BufferFull)?;

        let mut output = ArrayVec::new();

        loop {
            match Report::try_from(&self.buf[..]) {
                Ok(Some((report, bytes_read))) => {
                    self.buf.drain(0..bytes_read);
                    if output.len() < MAX_REPORT_QUEUE_LEN {
                        output.push(report);
                    } else {
                        return Err(Error::ReportQueueFull);
                    }
                },
                Err(_) => return Err(Error::MalformedMessage),
                Ok(None) => break,
            }
        }

        Ok(output)
    }
}

impl Default for ReportReader {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CommandReader {
    buf: ArrayVec<u8, MAX_SERIAL_MESSAGE_LEN>,
}

impl CommandReader {
    pub fn new() -> Self {
        Self { buf: ArrayVec::new() }
    }

    pub fn process_bytes<const MAX_COMMAND_QUEUE_LEN: usize>(
        &mut self,
        bytes: &[u8],
    ) -> Result<ArrayVec<Command, MAX_COMMAND_QUEUE_LEN>, Error> {
        self.buf.try_extend_from_slice(bytes).map_err(|_| Error::BufferFull)?;

        let mut output = ArrayVec::new();

        loop {
            match Command::try_from(&self.buf[..]) {
                Ok(Some((command, bytes_read))) => {
                    self.buf.drain(0..bytes_read);

                    if output.len() < MAX_COMMAND_QUEUE_LEN {
                        output.push(command);
                    } else {
                        return Err(Error::CommandQueueFull);
                    }
                },
                Err(_) => return Err(Error::MalformedMessage),
                Ok(None) => break,
            }
        }

        Ok(output)
    }
}

impl Default for CommandReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrips_arrayvec() {
        let commands = [
            Command::PowerCycler { slot: 1, state: true },
            Command::PowerCycler { slot: 20, state: false },
            Command::Temperature { target: 2, value: 100 },
            Command::Brightness { target: 10, value: 100 },
            Command::FanSpeed { target: 1, value: 600 },
            Command::Led { r: 0, g: 128, b: 255, pulse_mode: PulseMode::Solid },
            Command::Led { r: 0, g: 128, b: 255, pulse_mode: PulseMode::DialTurn },
            Command::Led {
                r: 0,
                g: 128,
                b: 255,
                pulse_mode: PulseMode::Breathing { interval_ms: NonZeroU16::new(4000).unwrap() },
            },
        ];

        for command in commands.iter() {
            let (deserialized, _len) =
                Command::try_from(&command.as_arrayvec()[..]).unwrap().unwrap();
            assert_eq!(command, &deserialized);
        }
    }

    #[test]
    fn report_roundtrips_arrayvec() {
        let reports = [
            Report::Press,
            Report::Release,
            Report::DialValue { diff: 100 },
            Report::EmergencyOff,
            Report::Error { code: 80 },
            Report::Debug { message: ArrayString::from("the frequency is 1000000000Hz").unwrap() },
        ];

        for report in reports.iter() {
            let (deserialized, _len) =
                Report::try_from(&report.as_arrayvec()[..]).unwrap().unwrap();
            assert_eq!(report, &deserialized);
        }
    }

    #[test]
    fn report_protocol_parse() {
        const REPORT_QUEUE_SIZE: usize = 6;

        let reports = [
            Report::Heartbeat,
            Report::Press,
            Report::Release,
            Report::DialValue { diff: 100 },
            Report::EmergencyOff,
            Report::Error { code: 80 },
        ];

        let mut protocol = ReportReader::new();
        for report_chunk in reports.chunks(REPORT_QUEUE_SIZE) {
            let mut bytes: ArrayVec<u8, MAX_SERIAL_MESSAGE_LEN> = ArrayVec::new();
            for report in report_chunk {
                bytes.try_extend_from_slice(&report.as_arrayvec()[..]).unwrap();
            }

            let report_output = protocol.process_bytes::<REPORT_QUEUE_SIZE>(&bytes).unwrap();

            assert_eq!(&report_output[..], report_chunk);
        }
    }

    #[test]
    fn command_protocol_parse() {
        const COMMAND_QUEUE_SIZE: usize = 6;

        let commands = [
            Command::PowerCycler { slot: 1, state: true },
            Command::PowerCycler { slot: 20, state: false },
            Command::Temperature { target: 2, value: 100 },
            Command::Brightness { target: 10, value: 100 },
            Command::Led { r: 0, g: 128, b: 255, pulse_mode: PulseMode::Solid },
            Command::Led { r: 0, g: 128, b: 255, pulse_mode: PulseMode::DialTurn },
            Command::Led {
                r: 0,
                g: 128,
                b: 255,
                pulse_mode: PulseMode::Breathing { interval_ms: NonZeroU16::new(4000).unwrap() },
            },
        ];

        let mut protocol = CommandReader::new();
        for command_chunk in commands.chunks(COMMAND_QUEUE_SIZE) {
            let mut bytes: ArrayVec<u8, MAX_SERIAL_MESSAGE_LEN> = ArrayVec::new();
            for command in command_chunk {
                bytes.try_extend_from_slice(&command.as_arrayvec()[..]).unwrap();
            }

            let command_output = protocol.process_bytes::<COMMAND_QUEUE_SIZE>(&bytes).unwrap();

            assert_eq!(&command_output[..], command_chunk);
        }
    }
}
