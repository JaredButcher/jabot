//! kv's rules, as pure functions.

use super::model::{Key, KvError};

/// Keys per page of a key list.
pub const PAGE_SIZE: usize = 25;
/// Longest key, in characters. Also the slash option's `max_length`.
pub const MAX_KEY_CHARS: usize = 64;
/// Longest value: the most a bot can send in one message.
pub const MAX_VALUE_CHARS: usize = 2000;
/// Most keys one user can have.
pub const MAX_KEYS: i64 = 1000;
/// Keys suggested while typing: the most Discord shows.
pub const SUGGESTIONS: i64 = 25;

/// Trim and lowercase `raw`, and check it's a usable key. Keys appear in inline code and in
/// custom ids, hence no backticks or control characters and the length limit.
pub fn normalize_key(raw: &str) -> Result<Key, KvError> {
    let key = raw.trim().to_lowercase();
    let length = key.chars().count();
    let valid =
        (1..=MAX_KEY_CHARS).contains(&length) && !key.chars().any(|c| c == '`' || c.is_control());
    if valid {
        Ok(Key(key))
    } else {
        Err(KvError::InvalidKey)
    }
}

/// Partial input for autocomplete, normalized the way keys are but not validated.
pub fn normalize_partial(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// What `/k get <query>` shows, given the keys containing `query`.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    Show(Key),
    List(Vec<String>),
    NotFound,
}

/// An exact match wins; otherwise a single match is shown, and several are listed.
pub fn resolve(query: &Key, mut matches: Vec<String>) -> Resolution {
    if matches.iter().any(|key| key == query.as_str()) {
        return Resolution::Show(query.clone());
    }
    match matches.len() {
        0 => Resolution::NotFound,
        1 => Resolution::Show(Key(matches.remove(0))),
        _ => Resolution::List(matches),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Page<'a> {
    pub keys: &'a [String],
    /// 0-based.
    pub number: usize,
    /// At least 1, so an empty list has one empty page.
    pub count: usize,
    pub total: usize,
}

impl Page<'_> {
    pub fn has_prev(&self) -> bool {
        self.number > 0
    }

    pub fn has_next(&self) -> bool {
        self.number + 1 < self.count
    }
}

/// Page `number` of `keys`, clamped to the last page, since the list may have shrunk since
/// the button was drawn.
pub fn paginate(keys: &[String], number: usize) -> Page<'_> {
    let count = keys.len().div_ceil(PAGE_SIZE).max(1);
    let number = number.min(count - 1);
    let start = number * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(keys.len());
    Page {
        keys: &keys[start..end],
        number,
        count,
        total: keys.len(),
    }
}

/// A DM's content is storable if it has text and fits in one message.
pub fn check_value(content: &str) -> Result<(), KvError> {
    let length = content.chars().count();
    if content.trim().is_empty() {
        Err(KvError::NoText)
    } else if length > MAX_VALUE_CHARS {
        Err(KvError::ValueTooLong(length))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(raw: &str) -> Key {
        normalize_key(raw).unwrap()
    }

    fn keys(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("k{i:03}")).collect()
    }

    #[test]
    fn keys_are_trimmed_and_lowercased() {
        assert_eq!(key("  Pasta ").as_str(), "pasta");
        assert_eq!(key("ÄRGER Über").as_str(), "ärger über");
        assert_eq!(key("a:b/c d").as_str(), "a:b/c d");
    }

    #[test]
    fn rejects_unusable_keys() {
        for bad in ["", "   ", "a`b", "a\nb", "tab\there"] {
            assert!(
                matches!(normalize_key(bad), Err(KvError::InvalidKey)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn key_length_counts_characters() {
        assert!(normalize_key(&"é".repeat(MAX_KEY_CHARS)).is_ok());
        assert!(normalize_key(&"a".repeat(MAX_KEY_CHARS + 1)).is_err());
    }

    #[test]
    fn exact_match_wins_over_others() {
        let matches = vec!["pasta".into(), "pasta sauce".into()];
        assert_eq!(
            resolve(&key("pasta"), matches),
            Resolution::Show(key("pasta"))
        );
    }

    #[test]
    fn single_partial_match_is_shown() {
        assert_eq!(
            resolve(&key("pas"), vec!["pasta".into()]),
            Resolution::Show(key("pasta"))
        );
    }

    #[test]
    fn several_partial_matches_are_listed() {
        let matches = vec!["pasta".into(), "spam".into()];
        assert_eq!(
            resolve(&key("pa"), matches.clone()),
            Resolution::List(matches)
        );
    }

    #[test]
    fn no_match_is_not_found() {
        assert_eq!(resolve(&key("x"), vec![]), Resolution::NotFound);
    }

    #[test]
    fn pages_hold_page_size_keys() {
        let keys = keys(PAGE_SIZE * 2 + 3);

        let first = paginate(&keys, 0);
        assert_eq!(first.keys, &keys[..PAGE_SIZE]);
        assert_eq!((first.number, first.count, first.total), (0, 3, keys.len()));
        assert!(!first.has_prev() && first.has_next());

        let last = paginate(&keys, 2);
        assert_eq!(last.keys, &keys[PAGE_SIZE * 2..]);
        assert!(last.has_prev() && !last.has_next());
    }

    #[test]
    fn page_past_the_end_is_clamped() {
        let keys = keys(PAGE_SIZE + 1);
        assert_eq!(paginate(&keys, 9).number, 1);
    }

    #[test]
    fn exact_multiple_of_page_size_has_no_empty_page() {
        assert_eq!(paginate(&keys(PAGE_SIZE * 2), 0).count, 2);
    }

    #[test]
    fn empty_list_has_one_empty_page() {
        let page = paginate(&[], 3);
        assert_eq!((page.number, page.count, page.total), (0, 1, 0));
        assert!(page.keys.is_empty());
        assert!(!page.has_prev() && !page.has_next());
    }

    #[test]
    fn values_need_text_and_must_fit_one_message() {
        assert!(matches!(check_value(""), Err(KvError::NoText)));
        assert!(matches!(check_value(" \n "), Err(KvError::NoText)));
        assert!(check_value(&"é".repeat(MAX_VALUE_CHARS)).is_ok());
        assert!(matches!(
            check_value(&"a".repeat(MAX_VALUE_CHARS + 1)),
            Err(KvError::ValueTooLong(2001))
        ));
    }
}
