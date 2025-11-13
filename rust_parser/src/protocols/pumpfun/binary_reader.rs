use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};
use thiserror::Error;

pub struct BinaryReader {
    buffer: Vec<u8>,
    offset: usize,
}

pub struct BinaryReaderRef<'a> {
    buffer: &'a [u8],
    offset: usize,
}

impl BinaryReader {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            buffer: data,
            offset: 0,
        }
    }

    pub fn read_fixed_array(&mut self, length: usize) -> Result<Vec<u8>, BinaryReaderError> {
        self.check_bounds(length)?;
        let slice = self.buffer[self.offset..self.offset + length].to_vec();
        self.offset += length;
        Ok(slice)
    }

    pub fn read_u8(&mut self) -> Result<u8, BinaryReaderError> {
        self.check_bounds(1)?;
        let value = self.buffer[self.offset];
        self.offset += 1;
        Ok(value)
    }

    pub fn read_u16(&mut self) -> Result<u16, BinaryReaderError> {
        self.check_bounds(2)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 2]);
        let value = cursor
            .read_u16::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 2;
        Ok(value)
    }

    pub fn read_u64(&mut self) -> Result<u64, BinaryReaderError> {
        self.check_bounds(8)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 8]);
        let value = cursor
            .read_u64::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 8;
        Ok(value)
    }

    pub fn read_i64(&mut self) -> Result<i64, BinaryReaderError> {
        self.check_bounds(8)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 8]);
        let value = cursor
            .read_i64::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 8;
        Ok(value)
    }

    pub fn read_string(&mut self) -> Result<String, BinaryReaderError> {
        self.check_bounds(4)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 4]);
        let length = cursor
            .read_u32::<LittleEndian>()
            .map_err(BinaryReaderError::Io)? as usize;
        self.offset += 4;
        self.check_bounds(length)?;
        let bytes = self.buffer[self.offset..self.offset + length].to_vec();
        self.offset += length;
        String::from_utf8(bytes).map_err(BinaryReaderError::InvalidString)
    }

    pub fn read_pubkey(&mut self) -> Result<String, BinaryReaderError> {
        let bytes = self.read_fixed_array(32)?;
        Ok(bs58::encode(bytes).into_string())
    }

    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.offset)
    }

    fn check_bounds(&self, length: usize) -> Result<(), BinaryReaderError> {
        if self.offset + length > self.buffer.len() {
            return Err(BinaryReaderError::BufferOverflow {
                length,
                offset: self.offset,
                buffer_len: self.buffer.len(),
            });
        }
        Ok(())
    }
}

impl<'a> BinaryReaderRef<'a> {
    pub fn new_ref(data: &'a [u8]) -> Self {
        Self {
            buffer: data,
            offset: 0,
        }
    }

    pub fn read_fixed_array(&mut self, length: usize) -> Result<Vec<u8>, BinaryReaderError> {
        self.check_bounds(length)?;
        let slice = self.buffer[self.offset..self.offset + length].to_vec();
        self.offset += length;
        Ok(slice)
    }

    pub fn read_u8(&mut self) -> Result<u8, BinaryReaderError> {
        self.check_bounds(1)?;
        let value = self.buffer[self.offset];
        self.offset += 1;
        Ok(value)
    }

    pub fn read_u16(&mut self) -> Result<u16, BinaryReaderError> {
        self.check_bounds(2)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 2]);
        let value = cursor
            .read_u16::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 2;
        Ok(value)
    }

    pub fn read_u64(&mut self) -> Result<u64, BinaryReaderError> {
        self.check_bounds(8)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 8]);
        let value = cursor
            .read_u64::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 8;
        Ok(value)
    }

    pub fn read_i64(&mut self) -> Result<i64, BinaryReaderError> {
        self.check_bounds(8)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 8]);
        let value = cursor
            .read_i64::<LittleEndian>()
            .map_err(BinaryReaderError::Io)?;
        self.offset += 8;
        Ok(value)
    }

    pub fn read_string(&mut self) -> Result<String, BinaryReaderError> {
        self.check_bounds(4)?;
        let mut cursor = Cursor::new(&self.buffer[self.offset..self.offset + 4]);
        let length = cursor
            .read_u32::<LittleEndian>()
            .map_err(BinaryReaderError::Io)? as usize;
        self.offset += 4;
        self.check_bounds(length)?;
        let bytes = self.buffer[self.offset..self.offset + length].to_vec();
        self.offset += length;
        String::from_utf8(bytes).map_err(BinaryReaderError::InvalidString)
    }

    pub fn read_pubkey(&mut self) -> Result<String, BinaryReaderError> {
        let bytes = self.read_fixed_array(32)?;
        Ok(bs58::encode(bytes).into_string())
    }

    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.offset)
    }

    fn check_bounds(&self, length: usize) -> Result<(), BinaryReaderError> {
        if self.offset + length > self.buffer.len() {
            return Err(BinaryReaderError::BufferOverflow {
                length,
                offset: self.offset,
                buffer_len: self.buffer.len(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum BinaryReaderError {
    #[error("buffer overflow: trying to read {length} bytes at offset {offset} from buffer of length {buffer_len}")]
    BufferOverflow {
        length: usize,
        offset: usize,
        buffer_len: usize,
    },
    #[error("failed to read value: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to read string: {0}")]
    InvalidString(#[from] std::string::FromUtf8Error),
}
