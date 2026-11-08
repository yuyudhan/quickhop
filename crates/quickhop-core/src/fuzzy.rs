//! Fuzzy matching: is the query a case-insensitive subsequence of a candidate, and how good
//! a match is it.
//!
//! The score rewards matches at the start of the candidate, at word starts (after a space,
//! `-`, `_`, `/`, `.` or at a lower-to-upper case change) and runs of consecutive matches, and
//! penalises skipped characters. The best alignment is found by dynamic programming over
//! query and candidate characters, so the result does not depend on greedy choices.

/// Score of every matched character.
const MATCH: i64 = 16;
/// Extra score when the match is the first character of the candidate.
const START_BONUS: i64 = 16;
/// Extra score when the match starts a word.
const WORD_BONUS: i64 = 12;
/// Extra score when the match starts a camel-case hump.
const CAMEL_BONUS: i64 = 10;
/// Extra score when the match directly follows the previous matched character.
const CONSECUTIVE_BONUS: i64 = 8;
/// Penalty per candidate character skipped between two matches.
const GAP_PENALTY: i64 = 1;
/// Penalty per candidate character before the first match, up to [`MAX_LEADING_PENALTY`].
const LEADING_PENALTY: i64 = 1;
/// Cap of the leading penalty, so long prefixes are not punished without bound.
const MAX_LEADING_PENALTY: i64 = 8;

/// A successful match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Higher is better; only comparable between matches of the same query.
    pub score: i64,
    /// Indices (in `char`s) of the candidate characters matched, strictly increasing.
    pub positions: Vec<usize>,
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '/' | '\\' | '.' | ':' | ',' | '(' | ')')
}

fn position_bonus(chars: &[char], index: usize) -> i64 {
    let Some(prev) = index.checked_sub(1).map(|i| chars[i]) else {
        return START_BONUS;
    };
    let current = chars[index];
    if is_separator(prev) {
        WORD_BONUS
    } else if prev.is_lowercase() && current.is_uppercase() {
        CAMEL_BONUS
    } else {
        0
    }
}

fn to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Matches `query` against `candidate`. Whitespace in the query is ignored, so `"fi fox"`
/// matches `"Firefox"`. Returns `None` when the query is not a subsequence. An empty query
/// matches everything with score 0.
pub fn score(query: &str, candidate: &str) -> Option<Match> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold)
        .collect();
    if needle.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    let original: Vec<char> = candidate.chars().collect();
    let hay: Vec<char> = original.iter().copied().map(fold).collect();
    let (n, m) = (needle.len(), hay.len());
    if n > m {
        return None;
    }
    // best[i][j]: best score with needle[i] matched at hay[j]; parent[i][j]: where needle[i-1]
    // was matched on that path.
    let mut best = vec![vec![None::<i64>; m]; n];
    let mut parent = vec![vec![0_usize; m]; n];
    for j in 0..m {
        if hay[j] == needle[0] {
            let leading = (to_i64(j) * LEADING_PENALTY).min(MAX_LEADING_PENALTY);
            best[0][j] = Some(MATCH + position_bonus(&original, j) - leading);
        }
    }
    for i in 1..n {
        // Running best of best[i-1][k] + GAP_PENALTY * k over k < j - 1, with its k.
        let mut running: Option<(i64, usize)> = None;
        for j in i..m {
            if j >= 2 {
                let k = j - 2;
                if let Some(prev) = best[i - 1][k] {
                    let value = prev + GAP_PENALTY * to_i64(k);
                    if running.is_none_or(|(r, _)| value > r) {
                        running = Some((value, k));
                    }
                }
            }
            if hay[j] != needle[i] {
                continue;
            }
            let bonus = MATCH + position_bonus(&original, j);
            let adjacent = best[i - 1][j - 1].map(|prev| (prev + CONSECUTIVE_BONUS, j - 1));
            let gapped = running.map(|(r, k)| (r - GAP_PENALTY * to_i64(j - 1), k));
            let chosen = match (adjacent, gapped) {
                (Some(a), Some(g)) => Some(if a.0 >= g.0 { a } else { g }),
                (a, g) => a.or(g),
            };
            if let Some((value, k)) = chosen {
                best[i][j] = Some(value + bonus);
                parent[i][j] = k;
            }
        }
    }
    let (end, total) = best[n - 1]
        .iter()
        .enumerate()
        .filter_map(|(j, s)| s.map(|s| (j, s)))
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    let mut positions = vec![0; n];
    let mut j = end;
    for i in (0..n).rev() {
        positions[i] = j;
        j = parent[i][j];
    }
    Some(Match {
        score: total,
        positions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(query: &str, candidate: &str) -> i64 {
        score(query, candidate).map_or(i64::MIN, |m| m.score)
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert_eq!(score("xyz", "Firefox"), None);
        assert_eq!(score("foxx", "fox"), None);
    }

    #[test]
    fn case_insensitive_and_whitespace_ignored() {
        let m = score("FI fox", "Firefox").unwrap();
        assert_eq!(m.positions, vec![0, 1, 4, 5, 6]);
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("", "anything").unwrap().score, 0);
    }

    #[test]
    fn prefix_beats_middle() {
        assert!(s("term", "Terminal") > s("term", "Xterm Terminal Emulator Settings"));
        assert!(s("code", "Code") > s("code", "Visual Studio Code"));
    }

    #[test]
    fn word_starts_beat_scattered() {
        assert!(s("vsc", "Visual Studio Code") > s("vsc", "avscanner"));
        assert!(s("gc", "GitClient") > s("gc", "Magic"));
    }

    #[test]
    fn picks_best_alignment_not_first() {
        // Greedy would take the first 'c'; the best alignment uses the word start.
        let m = score("sc", "scsi Studio Code").unwrap();
        assert_eq!(m.positions, vec![0, 1]);
        let m = score("code", "cxoxdxe Code").unwrap();
        assert_eq!(m.positions, vec![8, 9, 10, 11]);
    }
}
