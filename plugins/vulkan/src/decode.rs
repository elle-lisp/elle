use elle::value::Value;

/// Decode the output bytes from a GPU dispatch into Elle arrays.
///
/// Format:
///   4 bytes: output buffer count (u32 LE)
///   Per buffer: 4 bytes element count (u32 LE) + N*4 bytes data
///
/// Returns a single array if one output buffer, or array-of-arrays if multiple.
pub(crate) fn decode(bytes: &[u8], dtype: &str) -> Result<Value, String> {
    if bytes.len() < 4 {
        return Err("result bytes too short for header".into());
    }
    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let mut offset = 4;
    let mut arrays = Vec::with_capacity(count);

    for i in 0..count {
        if offset + 4 > bytes.len() {
            return Err(format!("truncated header for output buffer {i}"));
        }
        let n = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        let data_bytes = n * 4;
        if offset + data_bytes > bytes.len() {
            return Err(format!(
                "truncated data for output buffer {i}: need {data_bytes} bytes, have {}",
                bytes.len() - offset
            ));
        }

        let chunk = &bytes[offset..offset + data_bytes];
        let elements: Vec<Value> = match dtype {
            "f32" => chunk
                .chunks_exact(4)
                .map(|c| {
                    let f = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                    Value::float(f as f64)
                })
                .collect(),
            "u32" => chunk
                .chunks_exact(4)
                .map(|c| {
                    let n = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                    Value::int(n as i64)
                })
                .collect(),
            "i32" => chunk
                .chunks_exact(4)
                .map(|c| {
                    let n = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                    Value::int(n as i64)
                })
                .collect(),
            "raw" => {
                // Return raw bytes as a single bytes value per buffer
                arrays.push(Value::bytes(chunk.to_vec()));
                offset += data_bytes;
                continue;
            }
            _ => return Err(format!("unsupported dtype: {dtype:?}")),
        };

        arrays.push(Value::array(elements));
        offset += data_bytes;
    }

    if arrays.len() == 1 {
        Ok(arrays.into_iter().next().unwrap())
    } else {
        Ok(Value::array(arrays))
    }
}
