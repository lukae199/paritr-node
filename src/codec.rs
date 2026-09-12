//! Protocol 9's consensus codec.
//!
//! Integers are fixed-width little-endian. Variable byte strings and vectors
//! use a canonical `u32` element/byte count. Maps never appear on the wire.

use thiserror::Error;

pub const MAX_CONSENSUS_ALLOCATION: usize = 8 * 1024 * 1024;

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum CodecError {
    #[error("unexpected end of input")]
    EndOfInput,
    #[error("declared length {actual} exceeds limit {limit}")]
    LimitExceeded { actual: usize, limit: usize },
    #[error("invalid enum tag {0}")]
    InvalidTag(u8),
    #[error("non-canonical encoding: {0}")]
    NonCanonical(&'static str),
    #[error("trailing bytes ({0})")]
    TrailingBytes(usize),
    #[error("invalid UTF-8")]
    InvalidUtf8,
}

#[derive(Default, Debug, Clone)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity.min(MAX_CONSENSUS_ALLOCATION)),
        }
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn fixed(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub fn bytes(&mut self, value: &[u8]) {
        self.u32(u32::try_from(value.len()).expect("consensus value length fits u32"));
        self.fixed(value);
    }

    pub fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(CodecError::LimitExceeded {
                actual: usize::MAX,
                limit: self.bytes.len(),
            })?;
        if end > self.bytes.len() {
            return Err(CodecError::EndOfInput);
        }
        let out = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("fixed width"),
        ))
    }

    pub fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("fixed width"),
        ))
    }

    pub fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("fixed width"),
        ))
    }

    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        Ok(self.take(N)?.try_into().expect("fixed width"))
    }

    pub fn bytes(&mut self, max: usize) -> Result<&'a [u8], CodecError> {
        let len = usize::try_from(self.u32()?).expect("u32 fits usize");
        if len > max || len > MAX_CONSENSUS_ALLOCATION {
            return Err(CodecError::LimitExceeded {
                actual: len,
                limit: max.min(MAX_CONSENSUS_ALLOCATION),
            });
        }
        self.take(len)
    }

    pub fn string(&mut self, max: usize) -> Result<&'a str, CodecError> {
        std::str::from_utf8(self.bytes(max)?).map_err(|_| CodecError::InvalidUtf8)
    }

    pub fn vector_len(&mut self, max: usize) -> Result<usize, CodecError> {
        let len = usize::try_from(self.u32()?).expect("u32 fits usize");
        if len > max {
            return Err(CodecError::LimitExceeded {
                actual: len,
                limit: max,
            });
        }
        Ok(len)
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    pub fn finish(self) -> Result<(), CodecError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(CodecError::TrailingBytes(self.remaining()))
        }
    }
}

pub trait ConsensusEncode {
    fn encode_to(&self, writer: &mut Writer);

    fn consensus_encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.encode_to(&mut writer);
        writer.into_inner()
    }
}

pub trait ConsensusDecode: Sized {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError>;

    fn consensus_decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_CONSENSUS_ALLOCATION {
            return Err(CodecError::LimitExceeded {
                actual: bytes.len(),
                limit: MAX_CONSENSUS_ALLOCATION,
            });
        }
        let mut reader = Reader::new(bytes);
        let result = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_round_trip_and_trailing_rejection() {
        let mut writer = Writer::new();
        writer.u8(9);
        writer.u16(0x1234);
        writer.u32(0x5566_7788);
        writer.u64(0x1122_3344_5566_7788);
        writer.bytes(b"paritr");
        let encoded = writer.into_inner();

        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.u8().unwrap(), 9);
        assert_eq!(reader.u16().unwrap(), 0x1234);
        assert_eq!(reader.u32().unwrap(), 0x5566_7788);
        assert_eq!(reader.u64().unwrap(), 0x1122_3344_5566_7788);
        assert_eq!(reader.bytes(6).unwrap(), b"paritr");
        reader.finish().unwrap();
    }

    #[test]
    fn length_limit_is_checked_before_allocation() {
        let encoded_length = 100_u32.to_le_bytes();
        let mut reader = Reader::new(&encoded_length);
        assert!(matches!(
            reader.bytes(8),
            Err(CodecError::LimitExceeded {
                actual: 100,
                limit: 8
            })
        ));
    }
}
