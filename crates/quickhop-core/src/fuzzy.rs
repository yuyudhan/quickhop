//! Fuzzy matching with nucleo, the matcher used by the Helix editor.
//!
//! Every whitespace-separated word of the query must match the candidate as a
//! case-insensitive, Unicode-normalised subsequence; the score rewards matches at word
//! starts and consecutive runs. No special syntax (`^`, `$`, `!`) is interpreted.

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};

/// A successful match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Higher is better; only comparable between matches of the same query.
    pub score: u32,
    /// Sorted, unique `char` indices of the candidate that matched.
    pub positions: Vec<usize>,
}

/// A compiled query plus the matcher's scratch memory. Build one per query.
pub struct Matcher {
    pattern: Pattern,
    engine: nucleo_matcher::Matcher,
    chars: Vec<char>,
    indices: Vec<u32>,
}

impl Matcher {
    /// Compiles `query`.
    pub fn new(query: &str) -> Self {
        let mut config = Config::DEFAULT;
        config.prefer_prefix = true;
        Self {
            pattern: Pattern::new(
                query,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
            engine: nucleo_matcher::Matcher::new(config),
            chars: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Whether the query has no words, so everything matches with score 0.
    pub fn is_empty(&self) -> bool {
        self.pattern.atoms.is_empty()
    }

    /// Matches the query against `candidate`; `None` when some word does not match.
    pub fn score(&mut self, candidate: &str) -> Option<Match> {
        self.indices.clear();
        let haystack = Utf32Str::new(candidate, &mut self.chars);
        let score = self
            .pattern
            .indices(haystack, &mut self.engine, &mut self.indices)?;
        self.indices.sort_unstable();
        self.indices.dedup();
        let positions = self
            .indices
            .iter()
            .filter_map(|&i| usize::try_from(i).ok())
            .collect();
        Some(Match { score, positions })
    }
}

/// One-shot convenience: matches `query` against `candidate`.
pub fn score(query: &str, candidate: &str) -> Option<Match> {
    Matcher::new(query).score(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(query: &str, candidate: &str) -> u32 {
        score(query, candidate).map_or(0, |m| m.score)
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert_eq!(score("xyz", "Firefox"), None);
        assert_eq!(score("foxx", "fox"), None);
    }

    #[test]
    fn case_insensitive_and_words_matched_separately() {
        assert_eq!(score("FI", "Firefox").unwrap().positions, vec![0, 1]);
        assert_eq!(score("OX", "Firefox").unwrap().positions, vec![5, 6]);
        assert!(score("fox FI", "Firefox").is_some());
        assert!(score("fox zz", "Firefox").is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("", "anything").unwrap().score, 0);
        assert!(Matcher::new("  ").is_empty());
    }

    #[test]
    fn prefix_beats_middle() {
        assert!(s("term", "Terminal") > s("term", "Xterm Terminal Emulator Settings x"));
        assert!(s("code", "Code") > s("code", "Visual Studio Code"));
    }

    #[test]
    fn word_starts_beat_scattered() {
        assert!(s("vsc", "Visual Studio Code") > s("vsc", "avscanner"));
    }

    #[test]
    fn unicode_is_normalised() {
        assert!(score("cafe", "Café").is_some());
    }
}
