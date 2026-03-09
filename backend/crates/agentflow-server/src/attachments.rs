use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use lopdf::Document;

const MAX_ATTACHMENT_BYTES: usize = 1_000_000;
const MAX_ATTACHMENT_CHARS_PER_DOCUMENT: usize = 6_000;
const MAX_TOTAL_ATTACHMENT_CHARS: usize = 18_000;

pub trait AttachmentLike {
    fn name(&self) -> &str;
    fn content_type(&self) -> &str;
    fn data(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct AttachmentContext {
    pub name: String,
    pub content_type: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct AttachmentParseResult {
    pub contexts: Vec<AttachmentContext>,
    pub warnings: Vec<String>,
}

pub fn parse_attachments<T: AttachmentLike>(attachments: &[T]) -> AttachmentParseResult {
    let mut result = AttachmentParseResult::default();

    for attachment in attachments {
        match parse_attachment(attachment) {
            Ok(context) => result.contexts.push(context),
            Err(warning) => result.warnings.push(warning),
        }
    }

    result
}

pub fn render_attachment_context(contexts: &[AttachmentContext]) -> Option<String> {
    if contexts.is_empty() {
        return None;
    }

    let mut remaining = MAX_TOTAL_ATTACHMENT_CHARS;
    let mut blocks = Vec::new();

    for context in contexts {
        if remaining == 0 {
            break;
        }

        let text = truncate_chars(&context.text, remaining);
        let was_truncated =
            context.truncated || text.chars().count() < context.text.chars().count();

        blocks.push(format!(
            "[Document: {} | {}{}]\n{}",
            context.name,
            context.content_type,
            if was_truncated { " | truncated" } else { "" },
            text
        ));

        remaining = remaining.saturating_sub(text.chars().count());
    }

    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join("\n\n---\n\n"))
    }
}

fn parse_attachment<T: AttachmentLike>(attachment: &T) -> Result<AttachmentContext, String> {
    let bytes = BASE64_STANDARD
        .decode(attachment.data())
        .map_err(|e| format!("附件 {} base64 解码失败: {}", attachment.name(), e))?;

    if bytes.is_empty() {
        return Err(format!("附件 {} 为空，已忽略。", attachment.name()));
    }

    let text = if is_pdf_attachment(attachment.name(), attachment.content_type()) {
        extract_pdf_text(&bytes)
            .map_err(|e| format!("附件 {} PDF 文本提取失败: {}", attachment.name(), e))?
    } else if is_html_attachment(attachment.name(), attachment.content_type()) {
        strip_html_tags(&decode_text_bytes(&bytes))
    } else if is_text_attachment(attachment.name(), attachment.content_type()) {
        decode_text_bytes(&bytes)
    } else {
        return Err(format!(
            "附件 {} 暂不支持解析为文档上下文（{}）。",
            attachment.name(),
            attachment.content_type()
        ));
    };

    let normalized = normalize_text(&text);
    if normalized.is_empty() {
        return Err(format!(
            "附件 {} 未提取到可用文本，已忽略。",
            attachment.name()
        ));
    }

    let truncated = normalized.chars().count() > MAX_ATTACHMENT_CHARS_PER_DOCUMENT;
    let text = truncate_chars(&normalized, MAX_ATTACHMENT_CHARS_PER_DOCUMENT);

    Ok(AttachmentContext {
        name: attachment.name().to_string(),
        content_type: attachment.content_type().to_string(),
        text,
        truncated,
    })
}

fn extract_pdf_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "文件过大（{} bytes），当前上限 {} bytes",
            bytes.len(),
            MAX_ATTACHMENT_BYTES
        ));
    }

    let document = Document::load_mem(bytes).map_err(|e| e.to_string())?;
    let pages = document.get_pages();
    if pages.is_empty() {
        return Err("PDF 没有可读取页面".to_string());
    }

    let page_numbers: Vec<u32> = pages.keys().copied().collect();
    document
        .extract_text(&page_numbers)
        .map_err(|e| e.to_string())
}

fn decode_text_bytes(bytes: &[u8]) -> String {
    let trimmed = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };
    String::from_utf8_lossy(trimmed).into_owned()
}

fn is_text_attachment(name: &str, content_type: &str) -> bool {
    let lowered = content_type.to_lowercase();
    lowered.starts_with("text/")
        || matches!(
            lowered.as_str(),
            "application/json"
                | "application/ld+json"
                | "application/xml"
                | "text/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "text/yaml"
                | "text/csv"
                | "application/csv"
        )
        || matches!(
            extension(name).as_deref(),
            Some(
                "txt"
                    | "md"
                    | "markdown"
                    | "json"
                    | "yaml"
                    | "yml"
                    | "csv"
                    | "xml"
                    | "log"
                    | "rs"
                    | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "py"
                    | "toml"
            )
        )
}

fn is_html_attachment(name: &str, content_type: &str) -> bool {
    let lowered = content_type.to_lowercase();
    lowered == "text/html"
        || lowered == "application/xhtml+xml"
        || matches!(extension(name).as_deref(), Some("html" | "htm"))
}

fn is_pdf_attachment(name: &str, content_type: &str) -> bool {
    content_type.eq_ignore_ascii_case("application/pdf")
        || matches!(extension(name).as_deref(), Some("pdf"))
}

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

fn strip_html_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn normalize_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut last_blank = false;

    for raw_line in text.replace('\0', "").replace("\r\n", "\n").lines() {
        let line = raw_line.split_whitespace().collect::<Vec<_>>().join(" ");

        if line.is_empty() {
            if !last_blank {
                lines.push(String::new());
            }
            last_blank = true;
        } else {
            lines.push(line);
            last_blank = false;
        }
    }

    lines.join("\n").trim().to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let truncated: String = value.chars().take(max_chars).collect();
    format!("{truncated}\n...[truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestAttachment {
        name: String,
        content_type: String,
        data: String,
    }

    impl AttachmentLike for TestAttachment {
        fn name(&self) -> &str {
            &self.name
        }

        fn content_type(&self) -> &str {
            &self.content_type
        }

        fn data(&self) -> &str {
            &self.data
        }
    }

    #[test]
    fn parses_plain_text_attachment() {
        let attachment = TestAttachment {
            name: "notes.txt".to_string(),
            content_type: "text/plain".to_string(),
            data: BASE64_STANDARD.encode("Line one\nLine two"),
        };

        let result = parse_attachments(&[attachment]);
        assert!(result.warnings.is_empty());
        assert_eq!(result.contexts.len(), 1);
        assert!(result.contexts[0].text.contains("Line one"));
    }

    #[test]
    fn strips_html_when_extracting_text() {
        let attachment = TestAttachment {
            name: "page.html".to_string(),
            content_type: "text/html".to_string(),
            data: BASE64_STANDARD
                .encode("<html><body><h1>Title</h1><p>Hello <b>team</b></p></body></html>"),
        };

        let result = parse_attachments(&[attachment]);
        assert!(result.warnings.is_empty());
        assert!(result.contexts[0].text.contains("Title"));
        assert!(result.contexts[0].text.contains("Hello team"));
    }
}
