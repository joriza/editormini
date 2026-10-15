//! Fuzzy subsequence matching for the command palette and quick open.
//!
//! Zero dependencies on purpose: a case-insensitive subsequence test with a
//! light score (contiguity, word-start bonus, gap penalty) is enough to feel
//! like VS Code's pickers without pulling a fuzzy crate.

/// Score how well `query` matches `text` as a case-insensitive subsequence.
///
/// Returns `None` when the query is not a subsequence of the text. An empty
/// query matches everything with score 0. Scoring: +2 per matched character,
/// +5 when the match continues the previous one (contiguous run), +6 when the
/// matched character sits at a word start, −1 per skipped (gap) character.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.is_empty() {
        return Some(0);
    }
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i32;
    let mut cursor = 0usize; // next text index the scan may use
    let mut prev_matched: Option<usize> = None;
    for &qc in &query {
        let found = (cursor..lower.len()).find(|&i| lower[i] == qc)?;
        score += 2;
        if let Some(prev) = prev_matched {
            if found == prev + 1 {
                score += 5; // continues the previous match (contiguous run)
            } else {
                score -= (found - prev - 1) as i32; // skipped gap characters
            }
        }
        if is_word_start(&lower, found) {
            score += 6;
        }
        prev_matched = Some(found);
        cursor = found + 1;
    }
    Some(score)
}

/// Rank `items` against `query`: matching indices with their scores, best
/// first (score descending; ties keep the original order). Non-matching
/// items are omitted.
pub fn rank<'a, I>(query: &str, items: I) -> Vec<(usize, i32)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut scored: Vec<(usize, i32)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, text)| fuzzy_score(query, text).map(|s| (i, s)))
        .collect();
    // Stable sort: ties keep the original candidate order.
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored
}

fn is_word_start(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    !chars[index - 1].is_alphanumeric() && chars[index].is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything_with_zero() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("", ""), Some(0));
    }

    #[test]
    fn subsequence_matches_case_insensitively() {
        assert!(fuzzy_score("fnd", "Find").is_some());
        assert!(fuzzy_score("FIND", "find").is_some());
    }

    #[test]
    fn non_subsequence_is_none() {
        assert_eq!(fuzzy_score("fx", "Find"), None);
        assert_eq!(fuzzy_score("zx", "editor"), None);
    }

    #[test]
    fn contiguous_match_beats_gappy_match() {
        let contiguous = fuzzy_score("ab", "ab").unwrap();
        let gappy = fuzzy_score("ab", "axb").unwrap();
        assert!(contiguous > gappy, "{contiguous} must beat {gappy}");
    }

    #[test]
    fn word_start_match_beats_mid_word_match() {
        let at_start = fuzzy_score("s", "save all").unwrap();
        let mid_word = fuzzy_score("s", "paste").unwrap();
        assert!(at_start > mid_word, "{at_start} must beat {mid_word}");
    }

    #[test]
    fn rank_orders_by_score_desc_and_keeps_ties_in_order() {
        // "format": f at word start, o contiguous → 15.
        // "for":    f at word start, o contiguous → 15 (tie, later index).
        // "info":   f mid-word, o contiguous → 9.
        let ranked = rank("fo", ["format", "info", "for"]);
        assert_eq!(ranked, vec![(0, 15), (2, 15), (1, 9)]);
    }

    #[test]
    fn rank_requires_the_subsequence_in_order() {
        // "offset" has o BEFORE f, so the ordered subsequence "fo" is absent.
        assert_eq!(fuzzy_score("fo", "offset"), None);
    }

    #[test]
    fn rank_omits_non_matches() {
        let ranked = rank("zz", ["format", "info"]);
        assert!(ranked.is_empty());
    }
}
