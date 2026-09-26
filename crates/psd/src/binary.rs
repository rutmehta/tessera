use crate::{Error, Result};
pub(crate) fn error(s: &str) -> Error {
    Error(s.into())
}
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| error("length overflow"))?;
        let data = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| error("truncated PSD"))?;
        self.pos = end;
        Ok(data)
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.array()?))
    }
    pub fn length(&mut self, wide: bool) -> Result<usize> {
        let n = if wide {
            u64::from_be_bytes(self.array()?)
        } else {
            self.u32()? as u64
        };
        usize::try_from(n).map_err(|_| error("length exceeds address space"))
    }
    pub fn section(&mut self, wide: bool) -> Result<Reader<'a>> {
        let n = self.length(wide)?;
        Ok(Self::new(self.take(n)?))
    }
    pub fn blob(&mut self) -> Result<Vec<u8>> {
        let n = self.length(false)?;
        Ok(self.take(n)?.to_vec())
    }
    pub fn pascal(&mut self, align: usize) -> Result<Vec<u8>> {
        let n = self.u8()? as usize;
        let s = self.take(n)?.to_vec();
        self.take((align - (n + 1) % align) % align)?;
        Ok(s)
    }
    pub fn signature(&mut self, expected: &[u8; 4]) -> Result<()> {
        if &self.array::<4>()? != expected {
            return Err(error("invalid signature"));
        }
        Ok(())
    }
}
pub(crate) fn length(out: &mut Vec<u8>, n: usize, wide: bool) -> Result<()> {
    if wide {
        out.extend_from_slice(&(n as u64).to_be_bytes());
    } else {
        out.extend_from_slice(
            &u32::try_from(n)
                .map_err(|_| error("section exceeds PSD u32 length"))?
                .to_be_bytes(),
        );
    }
    Ok(())
}
pub(crate) fn section(out: &mut Vec<u8>, data: &[u8], wide: bool) -> Result<()> {
    length(out, data.len(), wide)?;
    out.extend_from_slice(data);
    Ok(())
}
pub(crate) fn pad(out: &mut Vec<u8>, align: usize) {
    let n = (align - out.len() % align) % align;
    out.resize(out.len() + n, 0);
}
pub(crate) fn pascal(out: &mut Vec<u8>, s: &[u8], align: usize) -> Result<()> {
    let n = u8::try_from(s.len()).map_err(|_| error("Pascal name exceeds 255 bytes"))?;
    out.push(n);
    out.extend_from_slice(s);
    out.resize(out.len() + (align - (s.len() + 1) % align) % align, 0);
    Ok(())
}
