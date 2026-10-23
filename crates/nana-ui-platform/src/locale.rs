//! The user's interface languages, and which of an application's languages
//! to show them.
//!
//! [`preferred_languages`] asks the platform once per call: macOS and iOS the
//! preferred-languages list, Windows the user's UI languages, Linux and other
//! Unix `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` in that order. The
//! application decides when to ask (usually at startup, when its own setting
//! says to follow the system) and keeps its choice in its own settings.
//!
//! [`negotiate_language`] picks from the languages an application ships. A
//! language matches only in a compatible script first, for every preferred
//! language in order; only when none does is a language taken in another
//! script. Chinese without a script names one by its region: Taiwan, Hong
//! Kong and Macau write Traditional (`Hant`), elsewhere Simplified (`Hans`).

/// The user's preferred interface languages as BCP 47 tags, most preferred
/// first (`zh-Hans-CN`, `en-US`, `ja-JP`). Empty when the platform names
/// none; the POSIX `C` locale names none.
pub fn preferred_languages() -> Vec<String> {
    let mut languages = Vec::new();
    for tag in sys_locale::get_locales() {
        let tag = tag.trim();
        if tag.is_empty() || tag.eq_ignore_ascii_case("c") || tag.eq_ignore_ascii_case("posix") {
            continue;
        }
        if !languages
            .iter()
            .any(|known: &String| known.eq_ignore_ascii_case(tag))
        {
            languages.push(tag.to_owned());
        }
    }
    languages
}

/// The language of `available` to show a user who prefers `preferred`, most
/// preferred first; `None` when no language of theirs is available.
///
/// A preferred language matches an available one with the same language and
/// script, best when the region also agrees. Every preferred language is
/// tried that way before any is taken in another script, so a user who reads
/// `zh-TW` and then `en` gets English over Simplified Chinese, and one who
/// reads only `zh-TW` gets Simplified Chinese over nothing.
pub fn negotiate_language<'a, P: AsRef<str>>(
    preferred: &[P],
    available: &[&'a str],
) -> Option<&'a str> {
    let available: Vec<(&'a str, Subtags)> = available
        .iter()
        .filter_map(|tag| Subtags::parse(tag).map(|subtags| (*tag, subtags)))
        .collect();
    let preferred: Vec<Subtags> = preferred
        .iter()
        .filter_map(|tag| Subtags::parse(tag.as_ref()))
        .collect();
    for wanted in &preferred {
        let same_script = available.iter().filter(|(_, offered)| {
            offered.language == wanted.language && offered.script == wanted.script
        });
        let best = same_script
            .clone()
            .find(|(_, offered)| offered.region.is_some() && offered.region == wanted.region)
            .or_else(|| {
                same_script
                    .clone()
                    .find(|(_, offered)| offered.region.is_none())
            })
            .or_else(|| same_script.clone().next());
        if let Some((tag, _)) = best {
            return Some(tag);
        }
    }
    preferred.iter().find_map(|wanted| {
        available
            .iter()
            .find(|(_, offered)| offered.language == wanted.language)
            .map(|(tag, _)| *tag)
    })
}

/// A tag's language, script and region, lowercased, with the script of
/// Chinese filled in from its region.
#[derive(Debug, PartialEq, Eq)]
struct Subtags {
    language: String,
    script: Option<String>,
    region: Option<String>,
}

impl Subtags {
    fn parse(tag: &str) -> Option<Self> {
        let mut parts = tag.split(['-', '_']).filter(|part| !part.is_empty());
        let language = parts.next()?.to_ascii_lowercase();
        if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        let mut script = None;
        let mut region = None;
        for part in parts {
            let part = part.to_ascii_lowercase();
            match part.len() {
                4 if script.is_none()
                    && region.is_none()
                    && part.bytes().all(|b| b.is_ascii_alphabetic()) =>
                {
                    script = Some(part);
                }
                2 | 3 if region.is_none() => region = Some(part),
                _ => break,
            }
        }
        if script.is_none() && language == "zh" {
            let traditional = matches!(region.as_deref(), Some("tw" | "hk" | "mo"));
            script = Some(if traditional { "hant" } else { "hans" }.to_owned());
        }
        Some(Self {
            language,
            script,
            region,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &[&str] = &["zh-Hans", "en", "ja"];

    fn pick(preferred: &[&str]) -> Option<&'static str> {
        negotiate_language(preferred, SHIPPED)
    }

    #[test]
    fn platform_spellings_match_the_shipped_languages() {
        assert_eq!(pick(&["zh-Hans-CN"]), Some("zh-Hans"));
        assert_eq!(pick(&["zh-CN"]), Some("zh-Hans"));
        assert_eq!(pick(&["zh_SG"]), Some("zh-Hans"));
        assert_eq!(pick(&["ja-JP"]), Some("ja"));
        assert_eq!(pick(&["en-GB"]), Some("en"));
        assert_eq!(pick(&["EN_us"]), Some("en"));
    }

    #[test]
    fn preference_order_wins_over_a_closer_later_match() {
        assert_eq!(pick(&["ja-JP", "zh-Hans-CN"]), Some("ja"));
        assert_eq!(pick(&["fr-FR", "de-DE", "en-US"]), Some("en"));
        assert_eq!(pick(&["fr-FR", "de-DE"]), None);
    }

    #[test]
    fn another_script_is_taken_only_when_no_preference_matches_in_its_own() {
        assert_eq!(pick(&["zh-TW", "en-US"]), Some("en"));
        assert_eq!(pick(&["zh-Hant-HK", "ja"]), Some("ja"));
        assert_eq!(pick(&["zh-TW"]), Some("zh-Hans"));
        assert_eq!(
            negotiate_language(&["zh-HK"], &["zh-Hans", "zh-Hant"]),
            Some("zh-Hant")
        );
    }

    #[test]
    fn a_matching_region_beats_the_bare_language() {
        let available = ["en", "en-GB", "pt-BR", "pt-PT"];
        assert_eq!(negotiate_language(&["en-GB"], &available), Some("en-GB"));
        assert_eq!(negotiate_language(&["en-AU"], &available), Some("en"));
        assert_eq!(negotiate_language(&["pt-PT"], &available), Some("pt-PT"));
        assert_eq!(negotiate_language(&["pt"], &available), Some("pt-BR"));
    }

    #[test]
    fn malformed_tags_match_nothing() {
        assert_eq!(pick(&["", "C", "x", "123"]), None);
        assert_eq!(negotiate_language(&["en"], &["", "english"]), None);
    }
}
