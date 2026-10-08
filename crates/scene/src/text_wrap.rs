//! Soft text wrapping, ported from `packages/element/src/textWrapping.ts` (`parseTokens`,
//! `wrapText`, `getWrappedTextLines`, `wrapLine`, `wrapWord`, `trimLine`,
//! `trimLineEndAtSoftBreak`) at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`. Widths come
//! from the caller's [`TextMeasure`]. Visual lines carry no source offsets here: only the
//! wrapped string is needed.

use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::color::is_js_whitespace;
use crate::text::TextMeasure;

// Character classes of `COMMON` and `CJK`, as literal members (the script classes are in
// `Classes::cjk_scripts`).
const HYPHEN: &str = "-";
const COMMON_OPENING: &str = "<([{";
const COMMON_CLOSING: &str = ">)]}.,:;!?…/";
const CJK_CHAR_LITERALS: &str = "｀＇＾〃〰〆＃＆＊＋－ー／＼＝｜￤〒￢￣";
const CJK_OPENING: &str = "（［｛〈《｟｢「『【〖〔〘〚＜〝";
const CJK_CLOSING: &str = "）］｝〉》｠｣」』】〗〕〙〛＞。．，、〟‥？！：；・〜〞";
const CJK_CURRENCY: &str = "￥￦￡￠＄";

struct Classes {
    /// `\p{Script=Han}`, Hiragana, Katakana and Hangul: the script part of `CJK.CHAR`.
    cjk_scripts: Regex,
    /// `getEmojiRegexUnicode()` anchored at the start of the haystack; group 1 is the match.
    emoji_anchored: Regex,
    /// The same pattern unanchored, for `wrapWord`'s `getEmojiRegex().test(word)`.
    emoji_anywhere: Regex,
}

fn classes() -> &'static Classes {
    static CLASSES: OnceLock<Classes> = OnceLock::new();
    CLASSES.get_or_init(|| {
        let flag = r"\p{Regional_Indicator}\p{Regional_Indicator}";
        let joiner = r"(?:\p{Emoji_Modifier}|\x{FE0F}\x{20E3}?|[\x{E0020}-\x{E007E}]+\x{E007F})?";
        let most = r"[\p{Extended_Pictographic}\p{Emoji_Presentation}]";
        let any = r"[\p{Emoji}]";
        let emoji = format!(r"({flag}|{most}{joiner}(?:\x{{200D}}(?:{flag}|{any}{joiner}))*)");
        Classes {
            cjk_scripts: Regex::new(
                r"^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]$",
            )
            .expect("script class"),
            emoji_anchored: Regex::new(&format!(r"\A{emoji}")).expect("emoji regex"),
            emoji_anywhere: Regex::new(&emoji).expect("emoji regex"),
        }
    })
}

fn is_cjk_char(c: char) -> bool {
    CJK_CHAR_LITERALS.contains(c) || classes().cjk_scripts.is_match(c.encode_utf8(&mut [0; 4]))
}

/// Whether a line break opportunity of `getLineBreakRegexAdvanced`'s zero-width rules lies
/// between `a` (the code point before, if any) and `b` (the one after, if any). A lookbehind
/// needs `a`; a negative lookahead holds at the end of the string.
fn zero_width_break(a: Option<char>, b: Option<char>) -> bool {
    let after = |f: &dyn Fn(char) -> bool| a.is_some_and(f);
    let before = |f: &dyn Fn(char) -> bool| b.is_some_and(f);
    let not_after = |f: &dyn Fn(char) -> bool| !a.is_some_and(f);
    let not_before = |f: &dyn Fn(char) -> bool| !b.is_some_and(f);
    let is_in = |set: &'static str| move |c: char| set.contains(c);
    let common_opening = is_in(COMMON_OPENING);
    let common_closing = is_in(COMMON_CLOSING);
    let cjk_opening = is_in(CJK_OPENING);
    let cjk_closing = is_in(CJK_CLOSING);

    // Break.Before(WHITESPACE)
    before(&is_js_whitespace)
        // Break.After(WHITESPACE, HYPHEN)
        || after(&|c| is_js_whitespace(c) || HYPHEN.contains(c))
        // Break.Before(CJK.CHAR, CJK.CURRENCY).NotPrecededBy(COMMON.OPENING, CJK.OPENING)
        || (not_after(&|c| common_opening(c) || cjk_opening(c))
            && before(&|c| is_cjk_char(c) || CJK_CURRENCY.contains(c)))
        // Break.After(CJK.CHAR).NotFollowedBy(HYPHEN, COMMON.CLOSING, CJK.CLOSING)
        || (after(&is_cjk_char)
            && not_before(&|c| HYPHEN.contains(c) || common_closing(c) || cjk_closing(c)))
        // Break.BeforeMany(CJK.OPENING).NotPrecededBy(COMMON.OPENING)
        || (not_after(&common_opening) && not_after(&cjk_opening) && before(&cjk_opening))
        // Break.AfterMany(CJK.CLOSING).NotFollowedBy(COMMON.CLOSING)
        || (after(&cjk_closing) && not_before(&cjk_closing) && not_before(&common_closing))
        // Break.AfterMany(COMMON.CLOSING).FollowedBy(COMMON.OPENING)
        || (after(&common_closing) && not_before(&common_closing) && before(&common_opening))
}

/// `parseTokens`: `line` (no `\n`) NFC-normalized and split at every line break opportunity
/// of `getLineBreakRegexAdvanced`; empty pieces dropped.
///
/// Follows `String.prototype.split(regex)`: a match that ends where the previous piece
/// started advances the search by one code point instead of cutting; an emoji match cuts
/// before and after itself and is kept as its own piece.
pub fn parse_tokens(line: &str) -> Vec<String> {
    let s: String = line.nfc().collect();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut tokens = Vec::new();
    let mut p = 0; // byte offset of the last cut
    let mut qi = 0; // index into `chars` of the search position
    while qi < chars.len() {
        let q = chars[qi].0;
        let emoji_len = classes().emoji_anchored.find(&s[q..]).map(|m| m.end());
        let end = match emoji_len {
            Some(len) => Some(q + len),
            None => {
                let a = qi.checked_sub(1).map(|i| chars[i].1);
                zero_width_break(a, Some(chars[qi].1)).then_some(q)
            }
        };
        match end {
            Some(e) if e != p => {
                tokens.push(s[p..q].to_owned());
                if let Some(len) = emoji_len {
                    tokens.push(s[q..q + len].to_owned());
                }
                p = e;
                qi = chars.partition_point(|&(byte, _)| byte < e);
            }
            _ => qi += 1,
        }
    }
    tokens.push(s[p..].to_owned());
    tokens.retain(|t| !t.is_empty());
    tokens
}

/// `wrapText` / `getWrappedTextLines(...).map(text).join("\n")`. A non-finite or negative
/// `max_width` splits only on existing `\n`.
pub fn wrap_text(
    text: &str,
    font_family: f64,
    font_size: f64,
    max_width: f64,
    measure: &mut dyn TextMeasure,
) -> String {
    if !max_width.is_finite() || max_width < 0.0 {
        return text.to_owned();
    }
    let mut lines: Vec<String> = Vec::new();
    for original in text.split('\n') {
        if measure.line_width(original, font_family, font_size) <= max_width {
            lines.push(original.to_owned());
        } else {
            wrap_line(
                original,
                font_family,
                font_size,
                max_width,
                measure,
                &mut lines,
            );
        }
    }
    lines.join("\n")
}

fn has_whitespace(s: &str) -> bool {
    s.chars().any(is_js_whitespace)
}

/// `isSingleCharacter`: exactly one UTF-16 code unit. `codePointAt(1)` of an astral
/// character is its low surrogate, so astral characters are not single.
fn is_single_character(s: &str) -> bool {
    let mut units = s.encode_utf16();
    units.next().is_some() && units.next().is_none()
}

fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

fn wrap_line(
    line: &str,
    font_family: f64,
    font_size: f64,
    max_width: f64,
    measure: &mut dyn TextMeasure,
    lines: &mut Vec<String>,
) {
    let tokens = parse_tokens(line);
    let mut current = String::new();
    let mut current_width = 0.0;
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        let test_line = format!("{current}{token}");
        // Single characters add their own width to the line's: no kerning applies.
        let test_width = if is_single_character(token) {
            current_width + measure.line_width(token, font_family, font_size)
        } else {
            measure.line_width(&test_line, font_family, font_size)
        };

        // Whitespace tokens join the line without a width check; they are trimmed later.
        if has_whitespace(token) || test_width <= max_width {
            current = test_line;
            current_width = test_width;
            i += 1;
        } else if current.is_empty() {
            // The token alone is wider than the line.
            let mut word_lines = wrap_word(token, font_family, font_size, max_width, measure);
            let trailing = word_lines.pop().unwrap_or_default();
            lines.append(&mut word_lines);
            current_width = measure.line_width(&trailing, font_family, font_size);
            current = trailing;
            i += 1;
        } else {
            // Soft break: the token is not consumed, it starts the next line.
            lines.push(trim_end(&current).to_owned());
            current.clear();
            current_width = 0.0;
        }
    }
    if !current.is_empty() {
        lines.push(trim_line(
            &current,
            font_family,
            font_size,
            max_width,
            measure,
        ));
    }
}

/// `wrapWord`: breaks a single token between code points; a token holding an emoji is
/// returned whole.
fn wrap_word(
    word: &str,
    font_family: f64,
    font_size: f64,
    max_width: f64,
    measure: &mut dyn TextMeasure,
) -> Vec<String> {
    if classes().emoji_anywhere.is_match(word) {
        return vec![word.to_owned()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0;
    for ch in word.chars() {
        let char_width = measure.line_width(ch.encode_utf8(&mut [0; 4]), font_family, font_size);
        let test_width = current_width + char_width;
        if test_width <= max_width {
            current.push(ch);
            current_width = test_width;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
        current_width = char_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `trimLine`: the last visual line of a hard line keeps trailing whitespace only while it
/// fits within `max_width`.
fn trim_line(
    line: &str,
    font_family: f64,
    font_size: f64,
    max_width: f64,
    measure: &mut dyn TextMeasure,
) -> String {
    if measure.line_width(line, font_family, font_size) <= max_width {
        return line.to_owned();
    }
    // `line.match(/^(.+?)(\s+)$/)`: the shortest non-empty prefix whose rest is whitespace.
    // `.` does not match line terminators.
    let chars: Vec<char> = line.chars().collect();
    let prefix_len = chars
        .iter()
        .rposition(|&c| !is_js_whitespace(c))
        .map_or(1, |last| last + 1);
    let is_terminator = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let (mut trimmed, whitespaces): (String, &[char]) =
        if prefix_len < chars.len() && !chars[..prefix_len].iter().any(|&c| is_terminator(c)) {
            (chars[..prefix_len].iter().collect(), &chars[prefix_len..])
        } else {
            (trim_end(line).to_owned(), &[])
        };
    let mut trimmed_width = measure.line_width(&trimmed, font_family, font_size);
    for &ws in whitespaces {
        let width = measure.line_width(ws.encode_utf8(&mut [0; 4]), font_family, font_size);
        if trimmed_width + width > max_width {
            break;
        }
        trimmed.push(ws);
        trimmed_width += width;
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::CharWidthMeasure;

    fn wrap(text: &str, max_width: f64) -> String {
        wrap_text(text, 5.0, 20.0, max_width, &mut CharWidthMeasure)
    }

    #[test]
    fn breaks_at_spaces_and_drops_the_space_at_the_soft_break() {
        assert_eq!(wrap("hello world", 70.0), "hello\nworld");
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_between_characters() {
        assert_eq!(wrap("abcdefghij", 50.0), "abcd\nefgh\nij");
    }

    #[test]
    fn trailing_spaces_on_the_last_line_are_kept_only_while_they_fit() {
        assert_eq!(wrap("ab   ", 40.0), "ab ");
    }

    #[test]
    fn cjk_breaks_between_any_two_characters() {
        assert_eq!(wrap("中文字", 30.0), "中文\n字");
    }

    #[test]
    fn hard_breaks_survive_and_a_bad_width_only_splits_on_them() {
        assert_eq!(wrap("a\nb", 100.0), "a\nb");
        assert_eq!(wrap("hello world", f64::NAN), "hello world");
        assert_eq!(wrap("hello world", -1.0), "hello world");
    }

    #[test]
    fn hyphen_breaks_after_and_whitespace_is_its_own_token() {
        assert_eq!(parse_tokens("Hello-world"), ["Hello-", "world"]);
        assert_eq!(parse_tokens("a  b"), ["a", " ", " ", "b"]);
    }
}
