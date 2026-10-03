//! Tiny search engine: every whitespace-separated token must match, either as
//! a case-insensitive substring of the whole entry, or (for short single-line
//! entries) as a fuzzy subsequence. A query typed in the wrong keyboard
//! layout (`пше` for `git`) is matched too.

use std::ops::Range;

const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";

pub struct Query {
    /// The query as typed, plus the same keys in the other layout.
    variants: Vec<Vec<Vec<char>>>,
}

fn tokens(q: &str) -> Vec<Vec<char>> {
    q.split_whitespace()
        .map(|t| t.chars().flat_map(char::to_lowercase).collect())
        .collect()
}

fn switch_layout(q: &str) -> String {
    q.chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if let Some(i) = EN.chars().position(|e| e == c) {
                RU.chars().nth(i).unwrap_or(c)
            } else if let Some(i) = RU.chars().position(|r| r == c) {
                EN.chars().nth(i).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

impl Query {
    pub fn new(q: &str) -> Self {
        let mut variants = vec![tokens(q)];
        let switched = tokens(&switch_layout(q));
        if switched != variants[0] {
            variants.push(switched);
        }
        Self { variants }
    }

    pub fn is_empty(&self) -> bool {
        self.variants[0].is_empty()
    }

    /// `haystack_lower` is the pre-lowercased search key of an entry,
    /// `title` its single-line preview. Higher score is better.
    pub fn score(&self, haystack_lower: &str, title: &str) -> Option<i64> {
        self.variants
            .iter()
            .enumerate()
            // Prefer the query as typed over the layout-switched one.
            .filter_map(|(i, v)| score_tokens(v, haystack_lower, title).map(|s| s - i as i64 * 200))
            .max()
    }

    /// Byte ranges in `title` to highlight.
    pub fn highlights(&self, title: &str) -> Vec<Range<usize>> {
        self.variants
            .iter()
            .map(|v| highlights(v, title))
            .find(|h| !h.is_empty())
            .unwrap_or_default()
    }
}

fn score_tokens(tokens: &[Vec<char>], haystack_lower: &str, title: &str) -> Option<i64> {
    let mut total = 0;
    for token in tokens {
        let needle: String = token.iter().collect();
        if let Some(pos) = haystack_lower.find(&needle) {
            let boundary = pos == 0
                || haystack_lower[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| !c.is_alphanumeric());
            total += 1000 - (pos.min(500) as i64) + if boundary { 300 } else { 0 };
        } else if title.chars().count() <= 200 {
            let (positions, gaps) = subsequence(title, token)?;
            if gaps > token.len() * 3 + 4 {
                return None;
            }
            total += 300 - (gaps as i64) * 10 - positions.first().copied().unwrap_or(0) as i64;
        } else {
            return None;
        }
    }
    Some(total)
}

fn highlights(tokens: &[Vec<char>], title: &str) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = title.char_indices().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|(_, c)| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut marked = vec![false; chars.len()];
    for token in tokens {
        if token.is_empty() || token.len() > lower.len() {
            continue;
        }
        let mut found = false;
        for start in 0..=lower.len() - token.len() {
            if lower[start..start + token.len()] == token[..] {
                marked[start..start + token.len()]
                    .iter_mut()
                    .for_each(|m| *m = true);
                found = true;
                break;
            }
        }
        if !found && let Some((positions, _)) = subsequence(title, token) {
            for p in positions {
                marked[p] = true;
            }
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (i, (byte, c)) in chars.iter().enumerate() {
        if !marked[i] {
            continue;
        }
        let end = byte + c.len_utf8();
        match ranges.last_mut() {
            Some(r) if r.end == *byte => r.end = end,
            _ => ranges.push(*byte..end),
        }
    }
    ranges
}

/// Greedy subsequence match; returns char positions and the total gap size.
fn subsequence(hay: &str, needle: &[char]) -> Option<(Vec<usize>, usize)> {
    let mut positions = Vec::with_capacity(needle.len());
    let mut it = needle.iter().peekable();
    for (i, c) in hay.chars().enumerate() {
        let Some(&&n) = it.peek() else { break };
        if c.to_lowercase().next() == Some(n) {
            positions.push(i);
            it.next();
        }
    }
    if it.peek().is_some() {
        return None;
    }
    let gaps = positions.windows(2).map(|w| w[1] - w[0] - 1).sum();
    Some((positions, gaps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_beats_fuzzy() {
        let q = Query::new("gpu");
        assert!(q.score("gpui.rs", "gpui.rs").unwrap() > q.score("g p u", "g p u").unwrap_or(0));
    }

    #[test]
    fn cyrillic_case_insensitive() {
        let q = Query::new("ПРИВ");
        assert!(q.score("привет мир", "Привет мир").is_some());
        assert_eq!(q.highlights("Привет мир"), vec![0.."Прив".len()]);
    }

    #[test]
    fn wrong_layout() {
        let q = Query::new("пше пгш");
        assert!(q.score("git gui", "git gui").is_some());
        assert_eq!(q.highlights("git gui"), vec![0..3, 4..7]);
        let q = Query::new("ghbdtn");
        assert!(q.score("привет", "привет").is_some());
    }

    #[test]
    fn all_tokens_required() {
        let q = Query::new("foo bar");
        assert!(q.score("foo baz", "foo baz").is_none());
        assert!(q.score("bar and foo", "bar and foo").is_some());
    }
}
