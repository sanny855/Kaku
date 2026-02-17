use config::keyassignment::PaneEncoding;
use encoding_rs::Encoding;

const MAX_TRAILING_ENCODED_BYTES: usize = 4;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum EscapeState {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Dcs,
    DcsEsc,
}

impl Default for EscapeState {
    fn default() -> Self {
        Self::Ground
    }
}

fn get_encoding(encoding: PaneEncoding) -> Option<&'static Encoding> {
    match encoding {
        PaneEncoding::Utf8 => None,
        PaneEncoding::Gbk => Some(encoding_rs::GBK),
        PaneEncoding::Gb18030 => Some(encoding_rs::GB18030),
        PaneEncoding::Big5 => Some(encoding_rs::BIG5),
        PaneEncoding::EucKr => Some(encoding_rs::EUC_KR),
        PaneEncoding::ShiftJis => Some(encoding_rs::SHIFT_JIS),
    }
}

fn advance_escape(state: EscapeState, byte: u8) -> EscapeState {
    match state {
        EscapeState::Ground => EscapeState::Ground,
        EscapeState::Esc => match byte {
            b'[' => EscapeState::Csi,
            b']' => EscapeState::Osc,
            b'P' => EscapeState::Dcs,
            0x40..=0x7e => EscapeState::Ground,
            _ => EscapeState::Esc,
        },
        EscapeState::Csi => {
            if matches!(byte, 0x40..=0x7e) {
                EscapeState::Ground
            } else {
                EscapeState::Csi
            }
        }
        EscapeState::Osc => match byte {
            0x07 => EscapeState::Ground,
            0x1b => EscapeState::OscEsc,
            _ => EscapeState::Osc,
        },
        EscapeState::OscEsc => {
            if byte == b'\\' {
                EscapeState::Ground
            } else {
                EscapeState::Osc
            }
        }
        EscapeState::Dcs => {
            if byte == 0x1b {
                EscapeState::DcsEsc
            } else {
                EscapeState::Dcs
            }
        }
        EscapeState::DcsEsc => {
            if byte == b'\\' {
                EscapeState::Ground
            } else {
                EscapeState::Dcs
            }
        }
    }
}

fn begin_escape(state: &mut EscapeState, escape_bytes: &mut Vec<u8>, byte: u8) {
    escape_bytes.clear();
    escape_bytes.push(byte);
    *state = if byte == 0x9b {
        EscapeState::Csi
    } else {
        EscapeState::Esc
    };
}

pub fn decode_bytes_to_string(encoding: PaneEncoding, raw: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(raw) {
        return text.to_string();
    }

    match get_encoding(encoding) {
        Some(enc) => {
            let (decoded, _, _) = enc.decode(raw);
            decoded.into_owned()
        }
        None => String::from_utf8_lossy(raw).into_owned(),
    }
}

#[derive(Debug)]
pub struct PaneInputEncoder {
    encoding: PaneEncoding,
    state: EscapeState,
    escape_bytes: Vec<u8>,
    pending_utf8: Vec<u8>,
}

impl Default for PaneInputEncoder {
    fn default() -> Self {
        Self {
            encoding: PaneEncoding::Utf8,
            state: EscapeState::Ground,
            escape_bytes: Vec::new(),
            pending_utf8: Vec::new(),
        }
    }
}

impl PaneInputEncoder {
    pub fn encode(&mut self, encoding: PaneEncoding, data: &[u8]) -> Vec<u8> {
        if self.encoding != encoding {
            self.encoding = encoding;
            self.state = EscapeState::Ground;
            self.escape_bytes.clear();
            self.pending_utf8.clear();
        }

        if encoding == PaneEncoding::Utf8 {
            return data.to_vec();
        }

        let mut output = Vec::with_capacity(data.len());
        let mut text_start = 0usize;

        for (idx, &byte) in data.iter().enumerate() {
            if self.state == EscapeState::Ground && (byte == 0x1b || byte == 0x9b) {
                if idx > text_start {
                    self.encode_text(encoding, &data[text_start..idx], &mut output);
                }
                begin_escape(&mut self.state, &mut self.escape_bytes, byte);
                text_start = idx + 1;
                continue;
            }

            if self.state != EscapeState::Ground {
                self.escape_bytes.push(byte);
                self.state = advance_escape(self.state, byte);

                if self.state == EscapeState::Ground {
                    output.extend_from_slice(&self.escape_bytes);
                    self.escape_bytes.clear();
                    text_start = idx + 1;
                }
            }
        }

        if self.state == EscapeState::Ground && text_start < data.len() {
            self.encode_text(encoding, &data[text_start..], &mut output);
        }

        output
    }

    fn encode_text(&mut self, encoding: PaneEncoding, text: &[u8], output: &mut Vec<u8>) {
        let mut pending = std::mem::take(&mut self.pending_utf8);
        pending.extend_from_slice(text);

        let mut cursor = 0usize;
        while cursor < pending.len() {
            match std::str::from_utf8(&pending[cursor..]) {
                Ok(valid) => {
                    self.push_encoded(encoding, valid, output);
                    return;
                }
                Err(err) => {
                    let valid_len = err.valid_up_to();
                    if valid_len > 0 {
                        let valid_slice = &pending[cursor..cursor + valid_len];
                        if let Ok(valid) = std::str::from_utf8(valid_slice) {
                            self.push_encoded(encoding, valid, output);
                        }
                    }

                    cursor += valid_len;
                    if err.error_len().is_none() {
                        self.pending_utf8.extend_from_slice(&pending[cursor..]);
                        return;
                    }

                    output.push(b'?');
                    cursor += err.error_len().unwrap_or(1);
                }
            }
        }
    }

    fn push_encoded(&self, encoding: PaneEncoding, text: &str, output: &mut Vec<u8>) {
        if let Some(enc) = get_encoding(encoding) {
            let (encoded, _, _) = enc.encode(text);
            output.extend_from_slice(&encoded);
        } else {
            output.extend_from_slice(text.as_bytes());
        }
    }
}

#[derive(Debug)]
pub struct PaneOutputDecoder {
    encoding: PaneEncoding,
    state: EscapeState,
    escape_bytes: Vec<u8>,
    pending_encoded: Vec<u8>,
}

impl Default for PaneOutputDecoder {
    fn default() -> Self {
        Self {
            encoding: PaneEncoding::Utf8,
            state: EscapeState::Ground,
            escape_bytes: Vec::new(),
            pending_encoded: Vec::new(),
        }
    }
}

impl PaneOutputDecoder {
    pub fn decode(&mut self, encoding: PaneEncoding, data: &[u8]) -> Vec<u8> {
        if self.encoding != encoding {
            self.encoding = encoding;
            self.state = EscapeState::Ground;
            self.escape_bytes.clear();
            self.pending_encoded.clear();
        }

        if encoding == PaneEncoding::Utf8 {
            return data.to_vec();
        }

        let mut output = Vec::with_capacity(data.len());
        let mut text_start = 0usize;

        for (idx, &byte) in data.iter().enumerate() {
            if self.state == EscapeState::Ground && (byte == 0x1b || byte == 0x9b) {
                if idx > text_start {
                    self.decode_text(encoding, &data[text_start..idx], &mut output);
                }
                begin_escape(&mut self.state, &mut self.escape_bytes, byte);
                text_start = idx + 1;
                continue;
            }

            if self.state != EscapeState::Ground {
                self.escape_bytes.push(byte);
                self.state = advance_escape(self.state, byte);
                if self.state == EscapeState::Ground {
                    output.extend_from_slice(&self.escape_bytes);
                    self.escape_bytes.clear();
                    text_start = idx + 1;
                }
            }
        }

        if self.state == EscapeState::Ground && text_start < data.len() {
            self.decode_text(encoding, &data[text_start..], &mut output);
        }

        output
    }

    fn decode_text(&mut self, encoding: PaneEncoding, input: &[u8], output: &mut Vec<u8>) {
        let mut pending = std::mem::take(&mut self.pending_encoded);
        pending.extend_from_slice(input);

        let Some(enc) = get_encoding(encoding) else {
            output.extend_from_slice(&pending);
            return;
        };

        let min_prefix = pending
            .len()
            .saturating_sub(MAX_TRAILING_ENCODED_BYTES)
            .max(1);

        for split in (min_prefix..=pending.len()).rev() {
            if let Some(decoded) =
                enc.decode_without_bom_handling_and_without_replacement(&pending[..split])
            {
                output.extend_from_slice(decoded.as_bytes());
                if split < pending.len() {
                    self.pending_encoded.extend_from_slice(&pending[split..]);
                }
                return;
            }
        }

        if pending.len() <= MAX_TRAILING_ENCODED_BYTES {
            self.pending_encoded.extend_from_slice(&pending);
            return;
        }

        let (decoded, _, _) = enc.decode(&pending);
        output.extend_from_slice(decoded.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_text(encoding: PaneEncoding, text: &str) {
        let mut encoder = PaneInputEncoder::default();
        let mut decoder = PaneOutputDecoder::default();
        let encoded = encoder.encode(encoding, text.as_bytes());
        let decoded = decoder.decode(encoding, &encoded);
        assert_eq!(decoded, text.as_bytes().to_vec());
    }

    #[test]
    fn utf8_passthrough() {
        let mut encoder = PaneInputEncoder::default();
        let mut decoder = PaneOutputDecoder::default();
        let data = "hello world".as_bytes();

        assert_eq!(encoder.encode(PaneEncoding::Utf8, data), data.to_vec());
        assert_eq!(decoder.decode(PaneEncoding::Utf8, data), data.to_vec());
    }

    #[test]
    fn supports_all_encodings_roundtrip() {
        round_trip_text(PaneEncoding::Gbk, "你好");
        round_trip_text(PaneEncoding::Gb18030, "你好世界");
        round_trip_text(PaneEncoding::Big5, "繁體中文");
        round_trip_text(PaneEncoding::EucKr, "안녕하세요");
        round_trip_text(PaneEncoding::ShiftJis, "こんにちは");
    }

    #[test]
    fn preserves_csi_esc_bracket_sequences() {
        let mut decoder = PaneOutputDecoder::default();
        let bytes = b"\x1b[31m";
        assert_eq!(decoder.decode(PaneEncoding::Gbk, bytes), bytes.to_vec());
    }

    #[test]
    fn preserves_csi_single_byte_sequences() {
        let mut decoder = PaneOutputDecoder::default();
        let bytes = [0x9b, b'3', b'1', b'm'];
        assert_eq!(decoder.decode(PaneEncoding::Gbk, &bytes), bytes.to_vec());
    }

    #[test]
    fn preserves_osc_and_dcs_sequences() {
        let mut decoder = PaneOutputDecoder::default();
        let osc = b"\x1b]0;title\x07";
        let dcs = b"\x1bPpayload\x1b\\";

        assert_eq!(decoder.decode(PaneEncoding::Gbk, osc), osc.to_vec());
        assert_eq!(decoder.decode(PaneEncoding::Gbk, dcs), dcs.to_vec());
    }

    #[test]
    fn mixed_text_and_escape_decode() {
        let mut decoder = PaneOutputDecoder::default();

        let mut data = vec![0xc4, 0xe3];
        data.extend_from_slice(b"\x1b[0m");
        data.extend_from_slice(&[0xba, 0xc3]);

        let result = decoder.decode(PaneEncoding::Gbk, &data);
        let mut expected = "你".as_bytes().to_vec();
        expected.extend_from_slice(b"\x1b[0m");
        expected.extend_from_slice("好".as_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn split_multibyte_decode_is_buffered() {
        let mut decoder = PaneOutputDecoder::default();

        let part1 = [0xc4];
        let result1 = decoder.decode(PaneEncoding::Gbk, &part1);
        assert!(result1.is_empty());

        let part2 = [0xe3];
        let result2 = decoder.decode(PaneEncoding::Gbk, &part2);
        assert_eq!(result2, "你".as_bytes().to_vec());
    }

    #[test]
    fn split_multibyte_encode_is_buffered() {
        let mut encoder = PaneInputEncoder::default();

        let first = [0xe4];
        let result1 = encoder.encode(PaneEncoding::Gbk, &first);
        assert!(result1.is_empty());

        let second = [0xbd, 0xa0];
        let result2 = encoder.encode(PaneEncoding::Gbk, &second);
        assert_eq!(result2, vec![0xc4, 0xe3]);
    }

    #[test]
    fn decode_bytes_to_string_works_for_utf8_and_non_utf8() {
        let utf8 = decode_bytes_to_string(PaneEncoding::Utf8, "hello世界".as_bytes());
        assert_eq!(utf8, "hello世界".to_string());

        let gbk_bytes = [0xc4, 0xe3, 0xba, 0xc3];
        let text = decode_bytes_to_string(PaneEncoding::Gbk, &gbk_bytes);
        assert_eq!(text, "你好".to_string());
    }
}
