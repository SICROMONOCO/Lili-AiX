//! "Did you mean" for title searches (Android's `TitleSpelling.kt`; keep the two in step, and
//! `fixtures/parity/spelling.json` is the contract between them).
//!
//! AniList has no answer for a misspelled word at all: probed 2026-10-06, `naurto`, `atack on
//! titan`, `jujutsu kaisn`, `freiren` and `one pice` each came back as an empty page, while every
//! correct spelling found its show. So a single slipped key ended on "Nothing found".
//!
//! The correction lines the typed words up against real titles rather than fixing each word alone.
//! Word by word, "one peace" is already fine because "peace" is a word; as a phrase, no title says
//! "one peace" and a very popular one says "one piece". A phrase some title already contains is
//! never corrected, which is what stops a correctly spelled but obscure title from being "fixed"
//! into a popular one. The last word may be the start of a title word, since search runs while the
//! viewer is still typing: "atack on ti" means Attack on Titan, and "naru" and "death not" are not
//! typos.
//!
//! Allowed slips grow with the word: none for one or two letters ("no", "x" and "on" are as likely
//! to be right as anything near them), one up to four letters, two from five. A swap of two
//! neighbouring letters counts as one slip. Among candidates as close as each other, the more
//! popular title wins.

use std::collections::HashMap;

/// A title the speller knows: the media index's `index.json` row, trimmed to what it reads.
#[derive(Debug, Clone)]
pub struct SpellingTitle {
    pub id: i64,
    pub title: String,
    pub popularity: i64,
    pub adult: bool,
}

/// What a misspelled search most likely meant: `text` to search instead, and the titles it names,
/// most watched first.
#[derive(Debug, Clone, PartialEq)]
pub struct Spelling {
    pub text: String,
    pub titles: Vec<i64>,
}

/// Past this, the query is a sentence rather than a title, and guessing at it is noise.
const MAX_QUERY_WORDS: usize = 8;

#[derive(Default)]
struct Word {
    /// How much this word is worth as a correction: the popularity of the titles using it.
    weight: i64,
    /// The same, counting only titles that are not adult, for when those are hidden.
    safe_weight: i64,
}

pub struct TitleSpeller {
    vocabulary: HashMap<String, Word>,
    /// Vocabulary words by length in characters, so a word is only compared with words that could
    /// be within reach of it.
    by_length: Vec<Vec<String>>,
    title_words: Vec<Vec<String>>,
    titles: Vec<SpellingTitle>,
}

impl TitleSpeller {
    pub fn new(titles: Vec<SpellingTitle>) -> Self {
        let mut vocabulary: HashMap<String, Word> = HashMap::new();
        let title_words: Vec<Vec<String>> = titles
            .iter()
            .map(|title| {
                let words = search_words(&title.title);
                let worth = title.popularity.max(0) + 1;
                for (at, word) in words.iter().enumerate() {
                    // Once per title, however often the title repeats it ("Hunter x Hunter").
                    if words.iter().position(|w| w == word) != Some(at) {
                        continue;
                    }
                    let entry = vocabulary.entry(word.clone()).or_default();
                    entry.weight += worth;
                    if !title.adult {
                        entry.safe_weight += worth;
                    }
                }
                words
            })
            .collect();
        let longest = vocabulary.keys().map(|w| w.chars().count()).max().unwrap_or(0);
        let mut by_length = vec![Vec::new(); longest + 1];
        for word in vocabulary.keys() {
            by_length[word.chars().count()].push(word.clone());
        }
        Self { vocabulary, by_length, title_words, titles }
    }

    /// The search `query` was most likely meant to be, or `None` when it is spelled like something
    /// the catalogue holds — or like nothing close enough to it to guess.
    pub fn suggest(&self, query: &str, hide_adult: bool, limit: usize) -> Option<Spelling> {
        let typed = search_words(query);
        if typed.is_empty() || typed.len() > MAX_QUERY_WORDS {
            return None;
        }
        let candidates: Vec<HashMap<&str, usize>> = typed.iter().map(|w| self.candidates_for(w, hide_adult)).collect();
        match self.phrase_in_titles(&typed, &candidates, hide_adult, limit) {
            Phrase::Spelled => None,
            Phrase::Corrected(spelling) => Some(spelling),
            Phrase::Unmatched => self.word_by_word(&typed, &candidates, hide_adult),
        }
    }

    /// The phrase as some title actually spells it.
    fn phrase_in_titles(&self, typed: &[String], candidates: &[HashMap<&str, usize>], hide_adult: bool, limit: usize) -> Phrase {
        let last = typed.last().map(String::as_str).unwrap_or_default();
        let mut best_cost = usize::MAX;
        let mut best_text: Option<String> = None;
        let mut best_popularity = i64::MIN;
        let mut matched: Vec<usize> = Vec::new();
        for (index, words) in self.title_words.iter().enumerate() {
            if (hide_adult && self.titles[index].adult) || words.len() < typed.len() {
                continue;
            }
            for start in 0..=words.len() - typed.len() {
                let mut cost = 0;
                let mut fits = true;
                for (at, candidate) in candidates.iter().enumerate() {
                    let word = words[start + at].as_str();
                    let slips =
                        if at == typed.len() - 1 && word.starts_with(last) { Some(0) } else { candidate.get(word).copied() };
                    match slips {
                        Some(slips) => cost += slips,
                        None => {
                            fits = false;
                            break;
                        }
                    }
                }
                if !fits || cost > best_cost {
                    continue;
                }
                if cost < best_cost {
                    best_cost = cost;
                    best_text = None;
                    best_popularity = i64::MIN;
                    matched.clear();
                }
                if matched.last() != Some(&index) {
                    matched.push(index);
                }
                let popularity = self.titles[index].popularity;
                if popularity > best_popularity {
                    best_popularity = popularity;
                    // A last word that only starts a title word is kept as typed: the viewer may
                    // not have meant the word this title happens to finish it with.
                    let text = (0..typed.len())
                        .map(|at| {
                            let word = words[start + at].as_str();
                            if at == typed.len() - 1 && word.starts_with(last) { typed[at].as_str() } else { word }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    best_text = Some(text);
                }
            }
        }
        // Spelled the way some title spells it: nothing to correct.
        if best_cost == 0 {
            return Phrase::Spelled;
        }
        let Some(text) = best_text else { return Phrase::Unmatched };
        matched.sort_by_key(|&i| std::cmp::Reverse(self.titles[i].popularity));
        Phrase::Corrected(Spelling { text, titles: matched.into_iter().take(limit).map(|i| self.titles[i].id).collect() })
    }

    /// Each unknown word on its own, for a phrase no title holds in that order: "naurto shippuden
    /// movie" is three right ideas, one misspelled, that no single title strings together.
    fn word_by_word(&self, typed: &[String], candidates: &[HashMap<&str, usize>], hide_adult: bool) -> Option<Spelling> {
        let corrected: Vec<String> = typed
            .iter()
            .enumerate()
            .map(|(at, word)| {
                if self.is_known(word, hide_adult) || (at == typed.len() - 1 && self.starts_any_word(word, hide_adult)) {
                    return word.clone();
                }
                candidates[at]
                    .iter()
                    .min_by(|a, b| {
                        a.1.cmp(b.1).then_with(|| self.worth(b.0, hide_adult).cmp(&self.worth(a.0, hide_adult))).then_with(|| a.0.cmp(b.0))
                    })
                    .map_or_else(|| word.clone(), |(w, _)| (*w).to_string())
            })
            .collect();
        (corrected != typed).then(|| Spelling { text: corrected.join(" "), titles: Vec::new() })
    }

    /// The catalogue's words within reach of `word`, each with how many slips away it is.
    fn candidates_for(&self, word: &str, hide_adult: bool) -> HashMap<&str, usize> {
        let mut found = HashMap::new();
        if let Some((known, _)) = self.vocabulary.get_key_value(word).filter(|(_, w)| worth_of(w, hide_adult) > 0) {
            found.insert(known.as_str(), 0);
        }
        let allowed = allowed_slips(word);
        if allowed == 0 {
            return found;
        }
        let length = word.chars().count();
        let shortest = length.saturating_sub(allowed).max(1);
        let longest = (length + allowed).min(self.by_length.len().saturating_sub(1));
        let typed: Vec<char> = word.chars().collect();
        for bucket in self.by_length.iter().take(longest + 1).skip(shortest) {
            for candidate in bucket {
                if candidate == word || self.worth(candidate, hide_adult) == 0 {
                    continue;
                }
                // Numbers are names here — "86", "2nd" — and one digit off is a different show.
                if candidate.chars().any(|c| c.is_numeric()) {
                    continue;
                }
                let other: Vec<char> = candidate.chars().collect();
                let slips = bounded_edit_distance(&typed, &other, allowed);
                if slips <= allowed {
                    found.insert(candidate.as_str(), slips);
                }
            }
        }
        found
    }

    fn worth(&self, word: &str, hide_adult: bool) -> i64 {
        self.vocabulary.get(word).map_or(0, |w| worth_of(w, hide_adult))
    }

    fn is_known(&self, word: &str, hide_adult: bool) -> bool {
        self.worth(word, hide_adult) > 0
    }

    fn starts_any_word(&self, prefix: &str, hide_adult: bool) -> bool {
        self.vocabulary.iter().any(|(text, w)| text.starts_with(prefix) && worth_of(w, hide_adult) > 0)
    }
}

enum Phrase {
    /// Some title holds the phrase as typed.
    Spelled,
    Corrected(Spelling),
    /// No title holds it in this order, however spelled.
    Unmatched,
}

fn worth_of(word: &Word, hide_adult: bool) -> i64 {
    if hide_adult { word.safe_weight } else { word.weight }
}

fn allowed_slips(word: &str) -> usize {
    let length = word.chars().count();
    if word.chars().any(|c| c.is_numeric()) || length <= 2 {
        0
    } else if length <= 4 {
        1
    } else {
        2
    }
}

/// A title or a query as the words a search compares: lowercased, split on anything that is not a
/// letter or digit, apostrophes dropped first so "Journey's" and "journeys" are the same word.
pub fn search_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else if c == '\'' || c == '’' {
            continue;
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Edits between `a` and `b` — insertions, deletions, substitutions and swaps of two neighbouring
/// letters — or `limit + 1` once it is certain to exceed `limit`, which is most of the vocabulary
/// for any one word and the reason this stops early.
pub fn bounded_edit_distance(a: &[char], b: &[char], limit: usize) -> usize {
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut before_previous = vec![0; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        let mut row_best = i;
        for j in 1..=b.len() {
            let substitution = usize::from(a[i - 1] != b[j - 1]);
            let mut value = (previous[j] + 1).min(current[j - 1] + 1).min(previous[j - 1] + substitution);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(before_previous[j - 2] + 1);
            }
            current[j] = value;
            row_best = row_best.min(value);
        }
        if row_best > limit {
            return limit + 1;
        }
        std::mem::swap(&mut before_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The cases both apps must answer alike (fixtures/parity/spelling.json).
    #[test]
    fn corrects_spelling_as_android_does() {
        let corpus: Value = serde_json::from_str(include_str!("../../../fixtures/parity/spelling.json")).unwrap();
        let titles = corpus["titles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| SpellingTitle {
                id: t["id"].as_i64().unwrap(),
                title: t["title"].as_str().unwrap().to_string(),
                popularity: t["popularity"].as_i64().unwrap(),
                adult: t["adult"].as_bool().unwrap(),
            })
            .collect();
        let speller = TitleSpeller::new(titles);
        let cases = corpus["cases"].as_array().unwrap();
        assert!(!cases.is_empty());
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|case| {
                let query = case["query"].as_str().unwrap();
                let answer = speller.suggest(query, case["hideAdult"].as_bool().unwrap(), 8);
                let expect = case["expect"].as_str();
                if answer.as_ref().map(|a| a.text.as_str()) != expect {
                    return Some(format!("{query:?}: {:?} (wanted {expect:?})", answer.map(|a| a.text)));
                }
                let offers: Option<Vec<i64>> = case["offers"].as_array().map(|o| o.iter().filter_map(Value::as_i64).collect());
                match (offers, answer) {
                    (Some(offers), Some(answer)) if answer.titles.iter().take(offers.len()).copied().collect::<Vec<_>>() != offers => {
                        Some(format!("{query:?}: offered {:?} (wanted {offers:?} first)", answer.titles))
                    }
                    _ => None,
                }
            })
            .collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_swap_of_neighbouring_letters_is_one_edit() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(bounded_edit_distance(&chars("naurto"), &chars("nauroto"), 2), 1);
        assert_eq!(bounded_edit_distance(&chars("freiren"), &chars("frieren"), 2), 1);
        assert_eq!(bounded_edit_distance(&chars("abc"), &chars("xyz123"), 2), 3);
    }

    #[test]
    fn words_ignore_case_punctuation_and_apostrophes() {
        assert_eq!(search_words("Frieren: Beyond Journey’s End"), ["frieren", "beyond", "journeys", "end"]);
        assert_eq!(search_words("Steins;Gate 0"), ["steins", "gate", "0"]);
    }
}
