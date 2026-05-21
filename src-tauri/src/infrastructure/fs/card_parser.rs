use base64::Engine;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardImageFormat {
    Png,
    WebP,
}

#[derive(Debug, Clone)]
pub struct ParsedCard {
    pub raw_json: Value,
    pub version_hint: Option<String>,
    pub avatar_bytes: Vec<u8>,
    pub avatar_mime: &'static str,
    pub format: CardImageFormat,
}

impl ParsedCard {
    pub fn avatar_extension(&self) -> &'static str {
        match self.format {
            CardImageFormat::Png => "png",
            CardImageFormat::WebP => "webp",
        }
    }
}

pub fn parse_sillytavern_card_image(bytes: &[u8]) -> Result<ParsedCard, String> {
    if is_png(bytes) {
        let raw_json = extract_sillytavern_json_from_png(bytes)?;
        return Ok(parsed_card(
            raw_json,
            bytes,
            "image/png",
            CardImageFormat::Png,
        ));
    }

    if is_webp(bytes) {
        let raw_json = extract_sillytavern_json_from_webp(bytes)?;
        return Ok(parsed_card(
            raw_json,
            bytes,
            "image/webp",
            CardImageFormat::WebP,
        ));
    }

    Err("Unsupported SillyTavern card image format".to_string())
}

fn parsed_card(
    raw_json: Value,
    bytes: &[u8],
    avatar_mime: &'static str,
    format: CardImageFormat,
) -> ParsedCard {
    let version_hint = infer_version_hint(&raw_json);
    ParsedCard {
        raw_json,
        version_hint,
        avatar_bytes: bytes.to_vec(),
        avatar_mime,
        format,
    }
}

fn infer_version_hint(card_json: &Value) -> Option<String> {
    let data = card_json.get("data").unwrap_or(card_json);
    string_field(card_json, "spec")
        .or_else(|| string_field(data, "spec"))
        .or_else(|| string_field(card_json, "spec_version"))
        .or_else(|| string_field(data, "spec_version"))
        .or_else(|| {
            string_field(data, "character_version").map(|v| format!("character_version:{v}"))
        })
}

fn string_field(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn is_png(bytes: &[u8]) -> bool {
    const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    bytes.len() >= PNG_SIG.len() && &bytes[..PNG_SIG.len()] == PNG_SIG
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn extract_sillytavern_json_from_png(bytes: &[u8]) -> Result<Value, String> {
    if !is_png(bytes) {
        return Err("Not a PNG file".to_string());
    }

    let mut offset = 8usize;
    let mut text_values = Vec::new();
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid PNG chunk length".to_string())?,
        ) as usize;
        offset += 4;
        if offset + 4 + length + 4 > bytes.len() {
            return Err("Truncated PNG chunk".to_string());
        }
        let chunk_type = &bytes[offset..offset + 4];
        offset += 4;
        let data = &bytes[offset..offset + length];
        offset += length + 4;

        if chunk_type == b"tEXt" {
            if let Some((keyword, text)) = split_png_text(data) {
                push_metadata_text(&mut text_values, keyword, text);
            }
        } else if chunk_type == b"iTXt" {
            if let Some((keyword, text)) = split_png_itxt(data) {
                push_metadata_text(&mut text_values, keyword, text);
            }
        }

        if chunk_type == b"IEND" {
            break;
        }
    }

    parse_first_card_json(text_values).ok_or_else(|| {
        "No SillyTavern character metadata found in PNG tEXt/iTXt chunks".to_string()
    })
}

fn extract_sillytavern_json_from_webp(bytes: &[u8]) -> Result<Value, String> {
    if !is_webp(bytes) {
        return Err("Not a WebP file".to_string());
    }

    let mut offset = 12usize;
    let mut text_values = Vec::new();
    while offset + 8 <= bytes.len() {
        let chunk_type = &bytes[offset..offset + 4];
        offset += 4;
        let length = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid WebP chunk length".to_string())?,
        ) as usize;
        offset += 4;
        if offset + length > bytes.len() {
            return Err("Truncated WebP chunk".to_string());
        }

        let data = &bytes[offset..offset + length];
        offset += length + (length % 2);

        if matches!(chunk_type, b"EXIF" | b"XMP " | b"ICCP") {
            collect_text_candidates(data, &mut text_values);
        }
    }

    parse_first_card_json(text_values).ok_or_else(|| {
        "No SillyTavern character metadata found in WebP EXIF/XMP chunks".to_string()
    })
}

fn push_metadata_text(text_values: &mut Vec<String>, keyword: String, text: String) {
    if keyword.eq_ignore_ascii_case("chara") {
        text_values.insert(0, text);
    } else {
        text_values.push(text);
    }
}

fn parse_first_card_json(text_values: Vec<String>) -> Option<Value> {
    for value in text_values {
        if let Ok(json) = parse_card_json_text(&value) {
            return Some(json);
        }
    }
    None
}

fn split_png_text(data: &[u8]) -> Option<(String, String)> {
    let nul = data.iter().position(|b| *b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..nul]).to_string();
    let text = String::from_utf8_lossy(&data[nul + 1..]).to_string();
    Some((keyword, text))
}

fn split_png_itxt(data: &[u8]) -> Option<(String, String)> {
    let keyword_end = data.iter().position(|b| *b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..keyword_end]).to_string();
    let mut idx = keyword_end + 1;
    if idx + 2 > data.len() {
        return None;
    }
    let compression_flag = data[idx];
    idx += 2; // flag + method
    let lang_end = data[idx..].iter().position(|b| *b == 0)? + idx;
    idx = lang_end + 1;
    let translated_end = data[idx..].iter().position(|b| *b == 0)? + idx;
    idx = translated_end + 1;
    if compression_flag != 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&data[idx..]).to_string();
    Some((keyword, text))
}

fn parse_card_json_text(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).map_err(|e| e.to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&decoded).map_err(|e| e.to_string())
}

fn collect_text_candidates(data: &[u8], text_values: &mut Vec<String>) {
    if let Ok(text) = std::str::from_utf8(data) {
        collect_candidates_from_text(text, text_values);
        return;
    }

    let lossy = String::from_utf8_lossy(data);
    collect_candidates_from_text(&lossy, text_values);
}

fn collect_candidates_from_text(text: &str, text_values: &mut Vec<String>) {
    text_values.push(text.trim_matches(char::from(0)).trim().to_string());

    for marker in ["chara", "ccv3", "SillyTavern", "spec"] {
        if let Some(idx) = text.find(marker) {
            collect_json_slice(&text[idx..], text_values);
            collect_base64_tokens(&text[idx + marker.len()..], text_values);
        }
    }

    collect_json_slice(text, text_values);
}

fn collect_json_slice(text: &str, text_values: &mut Vec<String>) {
    if let Some(start) = text.find('{') {
        if let Some(end) = find_balanced_json_end(&text[start..]) {
            text_values.push(text[start..start + end].to_string());
        }
    }
}

fn collect_base64_tokens(text: &str, text_values: &mut Vec<String>) {
    let mut token = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_') {
            token.push(ch);
            continue;
        }

        push_base64_token(&mut token, text_values);
    }

    push_base64_token(&mut token, text_values);
}

fn push_base64_token(token: &mut String, text_values: &mut Vec<String>) {
    if token.len() >= 12 {
        text_values.push(token.trim_matches('=').to_string());
        text_values.push(token.clone());
    }
    token.clear();
}

fn find_balanced_json_end(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_with_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(chunk_type);
        png.extend_from_slice(data);
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&0u32.to_be_bytes());
        png
    }

    fn webp_with_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let riff_size = 4 + 8 + data.len() + (data.len() % 2);
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&(riff_size as u32).to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(chunk_type);
        webp.extend_from_slice(&(data.len() as u32).to_le_bytes());
        webp.extend_from_slice(data);
        if data.len() % 2 == 1 {
            webp.push(0);
        }
        webp
    }

    #[test]
    fn parses_png_text_base64_payload() {
        let json = r#"{"spec":"chara_card_v2","data":{"name":"Ada"}}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        let mut data = b"chara\0".to_vec();
        data.extend_from_slice(encoded.as_bytes());

        let parsed = parse_sillytavern_card_image(&png_with_chunk(b"tEXt", &data)).unwrap();

        assert_eq!(parsed.raw_json["data"]["name"], "Ada");
        assert_eq!(parsed.version_hint.as_deref(), Some("chara_card_v2"));
        assert_eq!(parsed.avatar_mime, "image/png");
    }

    #[test]
    fn parses_png_itxt_plain_json_payload() {
        let mut data = b"chara\0\0\0\0\0".to_vec();
        data.extend_from_slice(br#"{"data":{"name":"Bert","character_version":"2.1"}}"#);

        let parsed = parse_sillytavern_card_image(&png_with_chunk(b"iTXt", &data)).unwrap();

        assert_eq!(parsed.raw_json["data"]["name"], "Bert");
        assert_eq!(
            parsed.version_hint.as_deref(),
            Some("character_version:2.1")
        );
    }

    #[test]
    fn rejects_png_without_metadata() {
        let err =
            parse_sillytavern_card_image(&png_with_chunk(b"tEXt", b"note\0hello")).unwrap_err();

        assert!(err.contains("No SillyTavern character metadata"));
    }

    #[test]
    fn parses_webp_xmp_chara_base64_payload() {
        let json = r#"{"data":{"name":"Webbie"}}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        let xmp = format!("<x:xmpmeta><chara>{encoded}</chara></x:xmpmeta>");

        let parsed =
            parse_sillytavern_card_image(&webp_with_chunk(b"XMP ", xmp.as_bytes())).unwrap();

        assert_eq!(parsed.raw_json["data"]["name"], "Webbie");
        assert_eq!(parsed.avatar_mime, "image/webp");
    }
}
