//! WhatsApp text formatting: `*bold*`, `_italic_`, `~strike~`, `` `code` ``, links and
//! mentions, turned into plain text plus styled ranges.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanStyle {
    Bold,
    Italic,
    Strike,
    Code,
    Link(String),
    /// A mention of a person; carries their JID.
    Mention(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte range in [`Rich::text`].
    pub range: Range<usize>,
    pub style: SpanStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rich {
    pub text: String,
    pub spans: Vec<Span>,
}

/// A mention to substitute: the digits that follow `@` in the raw text, the JID, and the
/// name to show.
#[derive(Clone, Debug)]
pub struct Mention {
    pub user: String,
    pub jid: String,
    pub name: String,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// Finds the closing marker for a formatting run opened at `open` (a char index).
fn closing(chars: &[char], open: usize, marker: char) -> Option<usize> {
    let first = *chars.get(open + 1)?;
    if first.is_whitespace() || first == marker {
        return None;
    }
    let mut index = open + 2;
    while index < chars.len() {
        let c = chars[index];
        if c == '\n' {
            return None;
        }
        if c == marker && !chars[index - 1].is_whitespace() {
            let after = chars.get(index + 1).copied();
            if after.is_none_or(|a| !is_word(a)) {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn emit(chars: &[char], mentions: &[Mention], out: &mut Rich) {
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let before_is_word = index > 0 && is_word(chars[index - 1]);

        // ```block``` and `inline` code: contents are literal.
        if c == '`' {
            let triple = chars.get(index + 1) == Some(&'`') && chars.get(index + 2) == Some(&'`');
            let (start, fence) = if triple { (index + 3, 3) } else { (index + 1, 1) };
            let mut end = start;
            let mut found = None;
            while end < chars.len() {
                let closes = if triple {
                    chars[end] == '`' && chars.get(end + 1) == Some(&'`') && chars.get(end + 2) == Some(&'`')
                } else {
                    chars[end] == '`'
                };
                if closes {
                    found = Some(end);
                    break;
                }
                if !triple && chars[end] == '\n' {
                    break;
                }
                end += 1;
            }
            if let Some(end) = found
                && end > start
            {
                let from = out.text.len();
                out.text.extend(chars[start..end].iter());
                out.spans.push(Span {
                    range: from..out.text.len(),
                    style: SpanStyle::Code,
                });
                index = end + fence;
                continue;
            }
        }

        if matches!(c, '*' | '_' | '~')
            && !before_is_word
            && let Some(close) = closing(chars, index, c)
        {
            let from = out.text.len();
            emit(&chars[index + 1..close], mentions, out);
            out.spans.push(Span {
                range: from..out.text.len(),
                style: match c {
                    '*' => SpanStyle::Bold,
                    '_' => SpanStyle::Italic,
                    _ => SpanStyle::Strike,
                },
            });
            index = close + 1;
            continue;
        }

        if c == '@' {
            let digits: String = chars[index + 1..]
                .iter()
                .take_while(|d| d.is_ascii_digit())
                .collect();
            if !digits.is_empty()
                && let Some(mention) = mentions.iter().find(|m| m.user == digits)
            {
                let from = out.text.len();
                out.text.push('@');
                out.text.push_str(&mention.name);
                out.spans.push(Span {
                    range: from..out.text.len(),
                    style: SpanStyle::Mention(mention.jid.clone()),
                });
                index += 1 + digits.len();
                continue;
            }
        }

        out.text.push(c);
        index += 1;
    }
}

fn find_links(text: &str, spans: &mut Vec<Span>) {
    let mut offset = 0;
    for token in text.split_inclusive(char::is_whitespace) {
        let word = token.trim_end_matches(char::is_whitespace);
        let lower = word.to_ascii_lowercase();
        let is_link = lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.");
        if is_link {
            let trimmed = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '\'']);
            if trimmed.len() > 7 || (lower.starts_with("www.") && trimmed.len() > 5) {
                let url = if lower.starts_with("www.") {
                    format!("https://{trimmed}")
                } else {
                    trimmed.to_string()
                };
                let range = offset..offset + trimmed.len();
                let inside_code = spans
                    .iter()
                    .any(|s| s.style == SpanStyle::Code && s.range.start <= range.start && range.end <= s.range.end);
                if !inside_code {
                    spans.push(Span {
                        range,
                        style: SpanStyle::Link(url),
                    });
                }
            }
        }
        offset += token.len();
    }
}

/// Parses message text into display text and styled ranges.
pub fn parse(input: &str, mentions: &[Mention]) -> Rich {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Rich::default();
    emit(&chars, mentions, &mut out);
    let text = out.text.clone();
    find_links(&text, &mut out.spans);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(rich: &Rich) -> Vec<(&str, SpanStyle)> {
        rich.spans
            .iter()
            .map(|s| (&rich.text[s.range.clone()], s.style.clone()))
            .collect()
    }

    #[test]
    fn plain_text_is_untouched() {
        let rich = parse("hello world", &[]);
        assert_eq!(rich.text, "hello world");
        assert!(rich.spans.is_empty());
    }

    #[test]
    fn markers_become_spans() {
        let rich = parse("a *bold* b _it_ c ~no~ d `x*y`", &[]);
        assert_eq!(rich.text, "a bold b it c no d x*y");
        assert_eq!(
            styles(&rich),
            vec![
                ("bold", SpanStyle::Bold),
                ("it", SpanStyle::Italic),
                ("no", SpanStyle::Strike),
                ("x*y", SpanStyle::Code),
            ]
        );
    }

    #[test]
    fn stray_markers_stay_literal() {
        assert_eq!(parse("2 * 3 * 4", &[]).text, "2 * 3 * 4");
        assert_eq!(parse("snake_case_name", &[]).text, "snake_case_name");
        assert_eq!(parse("*open only", &[]).text, "*open only");
        assert_eq!(parse("a*b*c", &[]).text, "a*b*c");
    }

    #[test]
    fn nesting_works() {
        let rich = parse("*bold _both_*", &[]);
        assert_eq!(rich.text, "bold both");
        assert_eq!(
            styles(&rich),
            vec![("both", SpanStyle::Italic), ("bold both", SpanStyle::Bold)]
        );
    }

    #[test]
    fn links_are_found_and_trimmed() {
        let rich = parse("see https://example.com/a?b=1, and www.test.org.", &[]);
        assert_eq!(
            styles(&rich),
            vec![
                ("https://example.com/a?b=1", SpanStyle::Link("https://example.com/a?b=1".into())),
                ("www.test.org", SpanStyle::Link("https://www.test.org".into())),
            ]
        );
    }

    #[test]
    fn mentions_show_names() {
        let mentions = [Mention {
            user: "15550001111".into(),
            jid: "15550001111@s.whatsapp.net".into(),
            name: "Ada".into(),
        }];
        let rich = parse("hi @15550001111!", &mentions);
        assert_eq!(rich.text, "hi @Ada!");
        assert_eq!(
            styles(&rich),
            vec![("@Ada", SpanStyle::Mention("15550001111@s.whatsapp.net".into()))]
        );
        // Unknown numbers are left as typed.
        assert_eq!(parse("@999", &mentions).text, "@999");
    }

    #[test]
    fn multibyte_text_keeps_valid_ranges() {
        let rich = parse("çay *içelim* 🙂 https://ö.example", &[]);
        for span in &rich.spans {
            assert!(rich.text.is_char_boundary(span.range.start));
            assert!(rich.text.is_char_boundary(span.range.end));
        }
        assert_eq!(&rich.text[rich.spans[0].range.clone()], "içelim");
    }
}
