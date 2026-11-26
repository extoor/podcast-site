//! String helpers: slugs, percent-encoding, cleaning tag text.

use std::collections::HashSet;

/// Cleans a string from a tag or a file name: control characters (including
/// line breaks and NUL) become spaces, runs of whitespace are collapsed.
/// An empty result → `None`.
pub fn clean(s: &str) -> Option<String> {
    let out = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!out.is_empty()).then_some(out)
}

/// Makes an ASCII slug: `[a-z0-9-]`, Cyrillic is transliterated,
/// other characters become `-`. An empty result → `"podcast"`.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if let Some(t) = translit(c) {
            out.push_str(t);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "podcast".to_owned()
    } else {
        out.to_owned()
    }
}

/// Returns `base`, or `base-2`, `base-3`, … on collision; records the result in `used`.
pub fn unique_slug(base: &str, used: &mut HashSet<String>) -> String {
    let mut slug = base.to_owned();
    let mut n = 2;
    while !used.insert(slug.clone()) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    slug
}

/// Transliteration of lowercase Russian and Ukrainian letters.
fn translit(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'ґ' => "g",
        'д' => "d",
        'е' => "e",
        'є' => "ye",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'і' => "i",
        'ї' => "yi",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' | 'ь' | '\'' | '’' => "",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        _ => return None,
    })
}

/// Percent-encodes one path segment (RFC 3986): only unreserved characters
/// `A-Z a-z 0-9 - . _ ~` are left unchanged.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Encodes a relative path segment by segment, keeping `/`.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_basic() {
        assert_eq!(slugify("KissFM"), "kissfm");
        assert_eq!(slugify("  Hello,  World!! "), "hello-world");
        assert_eq!(slugify("Радіо Їжак"), "radio-yizhak");
        assert_eq!(slugify("Щука"), "shchuka");
        assert_eq!(slugify("★★★"), "podcast");
    }

    #[test]
    fn slugs_are_unique() {
        let mut used = HashSet::new();
        assert_eq!(unique_slug("a", &mut used), "a");
        assert_eq!(unique_slug("a", &mut used), "a-2");
        assert_eq!(unique_slug("a", &mut used), "a-3");
    }

    #[test]
    fn encode() {
        assert_eq!(encode_segment("a b+c.mp3"), "a%20b%2Bc.mp3");
        assert_eq!(encode_segment("я"), "%D1%8F");
        assert_eq!(encode_path("Kiss FM/x?.mp3"), "Kiss%20FM/x%3F.mp3");
    }

    #[test]
    fn clean_text() {
        assert_eq!(clean(" a\r\nb\0 c ").as_deref(), Some("a b c"));
        assert_eq!(clean("\0\t "), None);
    }
}
