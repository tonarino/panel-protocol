#![cfg_attr(not(feature = "std"), no_std)]

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

impl Command {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Command, usize)>, ()> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'A', slot, state, ..] => Ok(Some((Command::PowerCycler { slot, state: state != 0 }, 3))),
            [b'B', msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Brightness { value }, 3)))
            },
            [b'C', msb, lsb, ..] => {
                let value = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Command::Temperature { value }, 3)))
            },
            _ => Err(()),
        }
    }

    #[cfg(feature = "arrayvec")]
    pub fn as_arrayvec(&self) -> arrayvec::ArrayVec<[u8; 8]> {
        let mut buf = arrayvec::ArrayVec::new();
        match *self {
            Command::PowerCycler { slot, state } => {
                buf.push(b'A');
                buf.push(slot);
                buf.push(u8::from(state));
            },
            Command::Brightness { value } => {
                buf.push(b'B');
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            },
            Command::Temperature { value } => {
                buf.push(b'C');
                buf.try_extend_from_slice(&value.to_be_bytes()).unwrap();
            },
        }
        buf
    }

    #[cfg(feature = "std")]
    pub fn as_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match *self {
            Command::PowerCycler { slot, state } => {
                buf.push(b'A');
                buf.push(slot);
                buf.push(u8::from(state));
            },
            Command::Brightness { value } => {
                buf.push(b'B');
                buf.extend_from_slice(&value.to_be_bytes());
            },
            Command::Temperature { value } => {
                buf.push(b'C');
                buf.extend_from_slice(&value.to_be_bytes());
            },
        }
        buf
    }
}

#[derive(Debug, PartialEq)]
pub enum Report {
    DialValue { diff: i8 },
    Click,
    EmergencyOff,
    Error { code: u16 },
}

impl Report {
    pub fn try_from(buf: &[u8]) -> Result<Option<(Report, usize)>, ()> {
        if buf.is_empty() {
            return Ok(None);
        }

        match *buf {
            [] => Ok(None),
            [b'A', diff, ..] => {
                let diff = i8::from_be_bytes([diff]);
                Ok(Some((Report::DialValue { diff }, 2)))
            },
            [b'B', ..] => Ok(Some((Report::Click, 1))),
            [b'C', ..] => Ok(Some((Report::EmergencyOff, 1))),
            [b'D', msb, lsb, ..] => {
                let code = u16::from_be_bytes([msb, lsb]);
                Ok(Some((Report::Error { code }, 3)))
            },
            _ => Err(()),
        }
    }

    #[cfg(feature = "arrayvec")]
    pub fn as_arrayvec(&self) -> arrayvec::ArrayVec<[u8; 8]> {
        let mut buf = arrayvec::ArrayVec::new();
        match *self {
            Report::DialValue { diff } => {
                buf.push(b'A');
                buf.try_extend_from_slice(&diff.to_be_bytes()).unwrap();
            },
            Report::Click => {
                buf.push(b'B');
            },
            Report::EmergencyOff => {
                buf.push(b'C');
            },
            Report::Error { code } => {
                buf.push(b'D');
                buf.try_extend_from_slice(&code.to_be_bytes()).unwrap();
            },
        }
        buf
    }

    #[cfg(feature = "std")]
    pub fn as_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match *self {
            Report::DialValue { diff } => {
                buf.push(b'A');
                buf.extend_from_slice(&diff.to_be_bytes());
            },
            Report::Click => {
                buf.push(b'B');
            },
            Report::EmergencyOff => {
                buf.push(b'C');
            },
            Report::Error { code } => {
                buf.push(b'D');
                buf.extend_from_slice(&code.to_be_bytes());
            },
        }
        buf
    }
}

#[cfg(all(not(feature = "arrayvec"), feature = "std"))]
pub struct Protocol {
    buf: Vec<u8>,
}

#[cfg(all(not(feature = "arrayvec"), feature = "std"))]
impl Protocol {
    pub fn process_byte(&mut self, byte: u8) -> Result<Option<Command>, Error> {
        self.buf.push(byte);
        match Command::try_from(&self.buf[..]) {
            Ok(Some((command, bytes_read))) => {
                self.buf.drain(0..bytes_read);
                Ok(Some(command))
            },
            Err(_) => Err(Error::MalformedMessage),
            Ok(None) => Ok(None),
        }
    }
}

#[cfg(feature = "arrayvec")]
pub struct Protocol {
    buf: arrayvec::ArrayVec<[u8; 256]>,
}

#[cfg(feature = "arrayvec")]
impl Protocol {
    pub fn process_byte(&mut self, byte: u8) -> Result<Option<Command>, Error> {
        self.buf.try_push(byte).map_err(|_| Error::BufferFull)?;
        match Command::try_from(&self.buf[..]) {
            Ok(Some((command, bytes_read))) => {
                self.buf.drain(0..bytes_read);
                Ok(Some(command))
            },
            Err(_) => Err(Error::MalformedMessage),
            Ok(None) => Ok(None),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "std")]
    fn command_roundtrips_vec() {
        let commands = [
            Command::PowerCycler { slot: 1, state: true },
            Command::PowerCycler { slot: 20, state: false },
            Command::Temperature { value: 100 },
            Command::Brightness { value: 100 },
        ];

        for command in commands.iter() {
            let (deserialized, _len) = Command::try_from(&command.as_vec()[..]).unwrap().unwrap();
            assert_eq!(command, &deserialized);
        }
    }

    #[test]
    #[cfg(feature = "arrayvec")]
    fn command_roundtrips_arrayvec() {
        let commands = [
            Command::PowerCycler { slot: 1, state: true },
            Command::PowerCycler { slot: 20, state: false },
            Command::Temperature { value: 100 },
            Command::Brightness { value: 100 },
        ];

        for command in commands.iter() {
            let (deserialized, _len) = Command::try_from(&command.as_arrayvec()[..]).unwrap().unwrap();
            assert_eq!(command, &deserialized);
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn report_roundtrips_vec() {
        let reports = [
            Report::Click,
            Report::DialValue { diff: 100 },
            Report::EmergencyOff,
            Report::Error { code: 80 },
        ];

        for report in reports.iter() {
            let (deserialized, _len) = Report::try_from(&report.as_vec()[..]).unwrap().unwrap();
            assert_eq!(report, &deserialized);
        }
    }

    #[test]
    #[cfg(feature = "arrayvec")]
    fn report_roundtrips_arrayvec() {
        let reports = [
            Report::Click,
            Report::DialValue { diff: 100 },
            Report::EmergencyOff,
            Report::Error { code: 80 },
        ];

        for report in reports.iter() {
            let (deserialized, _len) = Report::try_from(&report.as_arrayvec()[..]).unwrap().unwrap();
            assert_eq!(report, &deserialized);
        }
    }
}
