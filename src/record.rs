use crate::parser::ast::Value;

const TAG_INT: u8 = 1;
const TAG_STR: u8 = 2;

/// Serializes a row into raw bytes.
///
/// Layout:
///
/// ```text
/// [column count: u8] [one type tag per column: u8] [values...]
///   Int -> 8 bytes, little-endian
///   Str -> length as u32 little-endian, then the UTF-8 bytes
/// ```
fn encode(row: &[Value]) -> Vec<u8> {
    let count = u8::try_from(row.len()).expect("a row can have at most 255 columns");
    let mut bytes = vec![count];

    for value in row {
        bytes.push(match value {
            Value::Int(_) => TAG_INT,
            Value::Str(_) => TAG_STR,
        });
    }

    for value in row {
        match value {
            Value::Int(num) => bytes.extend_from_slice(&num.to_le_bytes()),
            Value::Str(string) => {
                let len = u32::try_from(string.len()).expect("string longer than 4 GiB");
                bytes.extend_from_slice(&len.to_le_bytes());
                bytes.extend_from_slice(string.as_bytes());
            }
        }
    }

    bytes
}

/// Deserializes bytes written by `encode` back into a row.
///
/// Errors if the bytes are cut short, have an unknown type tag, contain
/// invalid UTF-8, or have bytes left over after the last value.
fn decode(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let mut reader = Reader { bytes };

    let count = reader.take::<1>()?[0] as usize;
    let tags = reader.take_slice(count)?;

    let mut row = Vec::with_capacity(count);
    for &tag in tags {
        let value = match tag {
            TAG_INT => Value::Int(i64::from_le_bytes(reader.take()?)),
            TAG_STR => {
                let len = u32::from_le_bytes(reader.take()?) as usize;
                let raw = reader.take_slice(len)?;
                let string = std::str::from_utf8(raw).map_err(|e| format!("invalid text: {e}"))?;
                Value::Str(string.to_owned())
            }
            other => return Err(format!("unknown type tag {other}")),
        };
        row.push(value);
    }

    if !reader.bytes.is_empty() {
        return Err(format!(
            "{} unexpected bytes after record",
            reader.bytes.len()
        ));
    }
    Ok(row)
}

/// Reads from the front of a byte slice, failing instead of panicking when
/// there aren't enough bytes left.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take_slice(&mut self, n: usize) -> Result<&'a [u8], String> {
        let (head, rest) = self.bytes.split_at_checked(n).ok_or_else(|| {
            format!(
                "record cut short: wanted {n} bytes, {} left",
                self.bytes.len()
            )
        })?;
        self.bytes = rest;
        Ok(head)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take_slice(N)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    fn text(s: &str) -> Value {
        Value::Str(s.into())
    }

    fn round_trip(row: Vec<Value>) {
        assert_eq!(decode(&encode(&row)).unwrap(), row);
    }

    #[test]
    fn exact_bytes() {
        assert_eq!(
            encode(&[int(1), text("hi")]),
            vec![
                2, TAG_INT, TAG_STR, // count, tags
                1, 0, 0, 0, 0, 0, 0, 0, // 1 as i64
                2, 0, 0, 0, b'h', b'i', // len 2, "hi"
            ]
        );
    }

    #[test]
    fn round_trips() {
        round_trip(vec![]);
        round_trip(vec![int(0)]);
        round_trip(vec![int(-1), int(i64::MIN), int(i64::MAX)]);
        round_trip(vec![text("")]);
        round_trip(vec![text("héllo, 世界 🦀")]);
        round_trip(vec![int(1), text("alice"), int(30), text("")]);
    }

    #[test]
    fn many_columns() {
        round_trip((0..255).map(int).collect());
    }

    #[test]
    fn long_string() {
        round_trip(vec![text(&"x".repeat(10_000))]);
    }

    #[test]
    #[should_panic(expected = "at most 255 columns")]
    fn too_many_columns_panics() {
        let row: Vec<Value> = (0..256).map(int).collect();
        encode(&row);
    }

    #[test]
    fn empty_input_errors() {
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn every_truncation_errors() {
        let bytes = encode(&[int(7), text("hello")]);
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len]).is_err(), "cut to {len} bytes");
        }
    }

    #[test]
    fn trailing_bytes_error() {
        let mut bytes = encode(&[int(7)]);
        bytes.push(0);
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn unknown_tag_errors() {
        let err = decode(&[1, 99]).unwrap_err();
        assert!(err.contains("99"), "{err}");
    }

    #[test]
    fn invalid_utf8_errors() {
        let err = decode(&[1, TAG_STR, 2, 0, 0, 0, 0xFF, 0xFE]).unwrap_err();
        assert!(err.contains("invalid text"), "{err}");
    }

    #[test]
    fn huge_string_length_errors_without_allocating() {
        assert!(decode(&[1, TAG_STR, 0xFF, 0xFF, 0xFF, 0xFF]).is_err());
    }
}
