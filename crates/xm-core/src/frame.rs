//! Wire framing: start/end markers, escaping, checksum, and stream reassembly.

use thiserror::Error;

const START: u8 = 0x3E;
const END: u8 = 0x3C;
const ESC: u8 = 0x3D;
/// The most a frame may occupy while waiting for its end marker. The largest legitimate frame
/// seen is the device list at a few hundred bytes; past this the peer is not speaking the protocol.
const MAX_FRAME: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Ack = 0x01,
    DataTable1 = 0x0C,
    DataTable2 = 0x0E,
}

impl TryFrom<u8> for MessageType {
    type Error = FrameError;
    fn try_from(b: u8) -> Result<Self, FrameError> {
        match b {
            0x01 => Ok(Self::Ack),
            0x0C => Ok(Self::DataTable1),
            0x0E => Ok(Self::DataTable2),
            other => Err(FrameError::UnknownType(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: MessageType,
    pub seq: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameError {
    #[error("missing start or end marker")]
    BadMarkers,
    #[error("invalid escape sequence")]
    BadEscape,
    #[error("checksum mismatch")]
    BadChecksum,
    #[error("length field does not match payload")]
    BadLength,
    #[error("unknown message type {0:#04x}")]
    UnknownType(u8),
}

/// Sum of the bytes, modulo 256.
pub fn checksum(body: &[u8]) -> u8 {
    body.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

/// Escape the marker bytes `0x3C`, `0x3D`, `0x3E` as `0x3D` followed by the byte minus `0x10`.
pub fn escape(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for &b in raw {
        if matches!(b, END | ESC | START) {
            out.extend([ESC, b - 0x10]);
        } else {
            out.push(b);
        }
    }
    out
}

pub fn unescape(raw: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut out = Vec::with_capacity(raw.len());
    let mut it = raw.iter();
    while let Some(&b) = it.next() {
        if b == ESC {
            match it.next() {
                Some(&e @ 0x2C..=0x2E) => out.push(e + 0x10),
                _ => return Err(FrameError::BadEscape),
            }
        } else {
            out.push(b);
        }
    }
    Ok(out)
}

pub fn encode(frame: &Frame) -> Vec<u8> {
    let mut body = vec![frame.kind as u8, frame.seq];
    body.extend((frame.payload.len() as u32).to_be_bytes());
    body.extend(&frame.payload);
    body.push(checksum(&body));
    let mut out = vec![START];
    out.extend(escape(&body));
    out.push(END);
    out
}

pub fn decode(packed: &[u8]) -> Result<Frame, FrameError> {
    let inner = match packed {
        [START, inner @ .., END] => inner,
        _ => return Err(FrameError::BadMarkers),
    };
    let body = unescape(inner)?;
    // type, seq, 4-byte length, checksum is the smallest possible body
    if body.len() < 7 {
        return Err(FrameError::BadLength);
    }
    let (data, sum) = body.split_at(body.len() - 1);
    if checksum(data) != sum[0] {
        return Err(FrameError::BadChecksum);
    }
    let len = u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize;
    if len != data.len() - 6 {
        return Err(FrameError::BadLength);
    }
    Ok(Frame {
        kind: MessageType::try_from(data[0])?,
        seq: data[1],
        payload: data[6..].to_vec(),
    })
}

#[derive(Debug, Default)]
pub struct Reassembler {
    buf: Vec<u8>,
}

impl Reassembler {
    /// Feed bytes as they arrive; returns every frame (or frame error) completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Frame, FrameError>> {
        self.buf.extend(bytes);
        let mut out = Vec::new();
        loop {
            // Bytes before a start marker are garbage; a start marker never appears inside a frame.
            match self.buf.iter().position(|&b| b == START) {
                Some(0) => {}
                Some(n) => {
                    self.buf.drain(..n);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            let Some(end) = self.buf.iter().position(|&b| b == END) else {
                // No end in sight: cap what a peer can make us hold.
                if self.buf.len() > MAX_FRAME {
                    self.buf.clear();
                    out.push(Err(FrameError::BadLength));
                }
                break;
            };
            let packed: Vec<u8> = self.buf.drain(..=end).collect();
            // A later start marker means the earlier frame was cut short: resync on it.
            let from = packed.iter().rposition(|&b| b == START).unwrap_or(0);
            if from > 0 {
                out.push(Err(FrameError::BadMarkers));
            }
            out.push(decode(&packed[from..]));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_frame(payload: Vec<u8>, seq: u8) -> Frame {
        Frame {
            kind: MessageType::DataTable1,
            seq,
            payload,
        }
    }

    #[test]
    fn escape_rules() {
        let raw = [0x3C, 0x3D, 0x3E, 0x41];
        let esc = escape(&raw);
        assert_eq!(esc, [0x3D, 0x2C, 0x3D, 0x2D, 0x3D, 0x2E, 0x41]);
        assert_eq!(unescape(&esc).unwrap(), raw);
        assert_eq!(unescape(&[0x3D]), Err(FrameError::BadEscape));
    }

    #[test]
    fn encode_ack_seq0() {
        let f = Frame {
            kind: MessageType::Ack,
            seq: 0,
            payload: vec![],
        };
        assert_eq!(encode(&f), [0x3E, 0x01, 0x00, 0, 0, 0, 0, 0x01, 0x3C]);
    }

    #[test]
    fn encode_decode_round_trip_data() {
        let f = data_frame(
            vec![0x68, 0x19, 0x01, 0x01, 0x01, 0x01, 0x14, 0x00, 0x00],
            1,
        );
        let packed = encode(&f);
        assert_eq!(decode(&packed).unwrap(), f);
        let body = unescape(&packed[1..packed.len() - 1]).unwrap();
        let mut expect = vec![0x0C, 0x01, 0, 0, 0, 9];
        expect.extend(&f.payload);
        assert_eq!(&body[..body.len() - 1], expect.as_slice());
        assert_eq!(*body.last().unwrap(), checksum(&expect));
    }

    #[test]
    fn decode_rejects_bad_checksum() {
        let mut packed = encode(&data_frame(vec![1, 2, 3], 0));
        let i = packed.len() - 2;
        packed[i] ^= 0x01;
        assert_eq!(decode(&packed), Err(FrameError::BadChecksum));
    }

    #[test]
    fn decode_rejects_missing_markers() {
        let packed = encode(&data_frame(vec![1, 2, 3], 0));
        assert_eq!(decode(&packed[1..]), Err(FrameError::BadMarkers));
        assert_eq!(
            decode(&packed[..packed.len() - 1]),
            Err(FrameError::BadMarkers)
        );
    }

    #[test]
    fn reassembler_handles_split_and_merged() {
        let a = data_frame(vec![1, 2, 3], 0);
        let b = data_frame(vec![0x3C, 9], 1);
        let mut all = encode(&a);
        all.extend(encode(&b));
        for cut in 0..=all.len() {
            let mut r = Reassembler::default();
            let mut out = r.push(&all[..cut]);
            out.extend(r.push(&all[cut..]));
            assert_eq!(out, vec![Ok(a.clone()), Ok(b.clone())], "cut {cut}");
        }
    }

    #[test]
    fn reassembler_handles_escape_split_across_reads() {
        let f = data_frame(vec![0x10, 0x3C, 0x20], 3);
        let packed = encode(&f);
        let at = packed.iter().position(|&b| b == 0x3D).unwrap() + 1;
        let mut r = Reassembler::default();
        let mut out = r.push(&packed[..at]);
        assert!(out.is_empty());
        out.extend(r.push(&packed[at..]));
        assert_eq!(out, vec![Ok(f)]);
    }

    #[test]
    fn reassembler_drops_an_unterminated_frame_past_the_cap() {
        let mut r = Reassembler::default();
        let mut flood = vec![START];
        flood.extend(std::iter::repeat_n(0x01, MAX_FRAME + 100));
        let out = r.push(&flood);
        assert_eq!(out, [Err(FrameError::BadLength)]);
        // The buffer is clear again, so a good frame straight after decodes as usual.
        let good = encode(&data_frame(vec![0x01, 0x02], 0));
        assert_eq!(r.push(&good), [Ok(data_frame(vec![0x01, 0x02], 0))]);
    }

    #[test]
    fn reassembler_skips_garbage_before_start() {
        let f = data_frame(vec![5], 2);
        let mut bytes = vec![0x00, 0xFF];
        bytes.extend(encode(&f));
        assert_eq!(Reassembler::default().push(&bytes), vec![Ok(f)]);
    }
}
