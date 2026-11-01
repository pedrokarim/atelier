//! Traduction par l'API publique de MyMemory, sans clé.
//!
//! Le service détecte la langue source, mais refuse tout texte de plus de
//! 500 caractères : le texte est découpé sur ses fins de phrase, puis les
//! morceaux sont traduits par petits groupes et recollés dans l'ordre.
//!
//! Le texte part donc chez un tiers, et l'accès anonyme est limité à environ
//! 5 000 caractères par jour. La page Réglages le dit.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

use super::launcher::percent_encode;
use crate::http;

const ENDPOINT: &str = "https://api.mymemory.translated.net/get";

/// Longueur d'un morceau, avec une marge sur les 500 caractères autorisés.
const SEGMENT_CHARS: usize = 480;
/// Requêtes simultanées : assez pour qu'un long texte ne se fasse pas
/// attendre, assez peu pour rester correct envers un service gratuit.
const PARALLEL_REQUESTS: usize = 4;
const RESPONSE_LIMIT: usize = 256 * 1024;

/// Un texte traduit.
#[derive(Debug, PartialEq, Eq)]
pub struct Translation {
    pub text: String,
    /// Langue détectée ; absente si le texte était déjà dans la langue cible.
    pub source: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Translated {
        text: String,
        source: Option<String>,
    },
    /// Le service refuse de traduire un texte vers sa propre langue.
    AlreadyInTarget,
}

/// Un code de langue plausible : « fr », « en-US », « zh-CN »…
pub fn is_language(code: &str) -> bool {
    let mut parts = code.split('-');
    let primary = parts.next().unwrap_or_default();
    let region = parts.next();

    (2..=3).contains(&primary.len())
        && primary.bytes().all(|byte| byte.is_ascii_alphabetic())
        && region.is_none_or(|region| {
            (2..=4).contains(&region.len())
                && region.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        && parts.next().is_none()
}

/// Traduit `text` vers `target`, la langue source étant détectée. Un texte
/// déjà dans la langue cible est rendu tel quel.
pub fn translate(text: &str, target: &str) -> Result<Translation> {
    let segments = segments(text, SEGMENT_CHARS);
    let mut parts = Vec::with_capacity(segments.len());
    let mut source = None;

    for batch in segments.chunks(PARALLEL_REQUESTS) {
        let replies: Vec<Result<Reply>> = std::thread::scope(|scope| {
            let requests: Vec<_> = batch
                .iter()
                .map(|segment| scope.spawn(move || request(segment, target)))
                .collect();
            requests
                .into_iter()
                .map(|request| {
                    request
                        .join()
                        .unwrap_or_else(|_| Err(anyhow!("la traduction s’est interrompue")))
                })
                .collect()
        });

        for (segment, reply) in batch.iter().zip(replies) {
            match reply? {
                Reply::Translated {
                    text,
                    source: detected,
                } => {
                    parts.push(text);
                    source = source.or(detected);
                }
                Reply::AlreadyInTarget => parts.push(segment.clone()),
            }
        }
    }

    Ok(Translation {
        text: parts.join(" "),
        source,
    })
}

fn request(segment: &str, target: &str) -> Result<Reply> {
    let url = format!(
        "{ENDPOINT}?q={}&langpair={}",
        percent_encode(segment),
        percent_encode(&format!("Autodetect|{target}"))
    );
    let body = http::get(&url, "application/json", RESPONSE_LIMIT).context("traduction")?;
    parse_reply(&body)
}

/// Interprète une réponse. Les erreurs arrivent avec un statut HTTP 200, et
/// le message tient lieu de texte traduit : sans ce contrôle, la synthèse
/// vocale lirait l'avertissement à voix haute.
fn parse_reply(body: &[u8]) -> Result<Reply> {
    let reply: Value = serde_json::from_slice(body).context("réponse de traduction illisible")?;

    // Le statut est un nombre en cas de succès, une chaîne en cas d'erreur.
    let status = match &reply["responseStatus"] {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    };
    let data = &reply["responseData"];
    let text = data["translatedText"].as_str().unwrap_or_default();
    let details = reply["responseDetails"].as_str().unwrap_or_default();

    if status == Some(429) || text.starts_with("MYMEMORY WARNING") {
        return Err(anyhow!(
            "quota quotidien de traduction atteint\u{a0}: réessayez demain"
        ));
    }
    if details.contains("DISTINCT LANGUAGES") {
        return Ok(Reply::AlreadyInTarget);
    }
    if status != Some(200) || text.is_empty() {
        return Err(anyhow!("traduction refusée ({details})"));
    }

    Ok(Reply::Translated {
        text: decode_entities(text),
        source: data["detectedLanguage"]
            .as_str()
            .filter(|language| !language.is_empty())
            .map(str::to_string),
    })
}

/// Découpe un texte en morceaux d'au plus `limit` caractères : sur une fin
/// de phrase si possible, sinon sur un espace, en dernier recours n'importe
/// où.
fn segments(text: &str, limit: usize) -> Vec<String> {
    let mut rest = text.trim();
    let mut found = Vec::new();

    while !rest.is_empty() {
        let Some((window_end, _)) = rest.char_indices().nth(limit) else {
            found.push(rest.to_string());
            break;
        };

        let cut = cut_point(&rest[..window_end]);
        found.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }

    found.retain(|segment| !segment.is_empty());
    found
}

/// Où couper `window` : juste après sa dernière phrase, sinon après son
/// dernier espace, sinon à sa fin.
fn cut_point(window: &str) -> usize {
    let mut sentence = None;
    let mut space = None;
    let mut characters = window.char_indices().peekable();

    while let Some((index, character)) = characters.next() {
        let end = index + character.len_utf8();
        let before_space = characters
            .peek()
            .is_some_and(|(_, next)| next.is_whitespace());

        // Les ponctuations asiatiques ne sont pas suivies d'un espace ; une
        // ponctuation latine doit l'être, sans quoi « 3.14 » serait une fin
        // de phrase.
        if matches!(character, '\n' | '。' | '！' | '？')
            || (matches!(character, '.' | '!' | '?' | '…') && before_space)
        {
            sentence = Some(end);
        } else if character.is_whitespace() {
            space = Some(end);
        }
    }

    sentence.or(space).unwrap_or(window.len())
}

/// Le service rend parfois des entités HTML (`l&#39;ami`) : lues telles
/// quelles, elles seraient épelées.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];

        let entity = rest[1..].find(';').and_then(|end| {
            let character = entity_character(&rest[1..1 + end])?;
            Some((character, end + 2))
        });
        match entity {
            Some((character, length)) => {
                decoded.push(character);
                rest = &rest[length..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }

    decoded.push_str(rest);
    decoded
}

fn entity_character(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_language_codes_only() {
        for code in ["fr", "en-US", "zh-CN", "ckb", "sr-Latn"] {
            assert!(is_language(code), "{code}");
        }
        for code in ["", "f", "français", "fr_FR", "fr-", "fr-FR-x", "f1"] {
            assert!(!is_language(code), "{code}");
        }
    }

    #[test]
    fn short_text_stays_whole() {
        assert_eq!(segments("  Une phrase.  ", 480), ["Une phrase."]);
        assert!(segments("   ", 480).is_empty());
    }

    #[test]
    fn cuts_on_sentence_ends() {
        let found = segments("Une phrase. Une autre phrase. Et une dernière.", 32);
        assert_eq!(found, ["Une phrase. Une autre phrase.", "Et une dernière."]);
    }

    #[test]
    fn decimal_points_do_not_end_sentences() {
        assert_eq!(cut_point("Il vaut 3.14 environ"), "Il vaut 3.14 ".len());
    }

    #[test]
    fn cuts_on_asian_punctuation_without_spaces() {
        let found = segments("今日は晴れです。公園を散歩しました。", 12);
        assert_eq!(found, ["今日は晴れです。", "公園を散歩しました。"]);
    }

    #[test]
    fn text_without_break_is_cut_at_the_limit() {
        let found = segments(&"é".repeat(25), 10);
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|segment| segment.chars().count() <= 10));
        assert_eq!(found.concat(), "é".repeat(25));
    }

    #[test]
    fn no_segment_exceeds_the_service_limit() {
        let text = "Une phrase ordinaire, assez longue pour se répéter. ".repeat(200);
        let found = segments(&text, SEGMENT_CHARS);
        assert!(found.iter().all(|segment| segment.chars().count() <= 500));
        assert_eq!(found.join(" "), text.trim());
    }

    #[test]
    fn reads_a_translation() {
        let body = br#"{"responseData":{"translatedText":"L&#39;ami &amp; moi","detectedLanguage":"en"},"responseStatus":200,"responseDetails":""}"#;
        assert_eq!(
            parse_reply(body).unwrap(),
            Reply::Translated {
                text: "L'ami & moi".to_string(),
                source: Some("en".to_string()),
            }
        );
    }

    #[test]
    fn same_language_is_not_an_error() {
        let body = br#"{"responseData":{"translatedText":"PLEASE SELECT TWO DISTINCT LANGUAGES"},"responseDetails":"PLEASE SELECT TWO DISTINCT LANGUAGES","responseStatus":"403"}"#;
        assert_eq!(parse_reply(body).unwrap(), Reply::AlreadyInTarget);
    }

    #[test]
    fn service_messages_are_never_read_aloud() {
        let quota = br#"{"responseData":{"translatedText":"MYMEMORY WARNING: YOU USED ALL AVAILABLE FREE TRANSLATIONS FOR TODAY."},"responseStatus":429,"responseDetails":""}"#;
        assert!(
            parse_reply(quota)
                .unwrap_err()
                .to_string()
                .contains("quota")
        );

        let refused = br#"{"responseData":{"translatedText":"QUERY LENGTH LIMIT EXCEEDED"},"responseDetails":"QUERY LENGTH LIMIT EXCEEDED","responseStatus":"403"}"#;
        assert!(parse_reply(refused).is_err());
    }

    #[test]
    fn broken_entities_stay_as_they_are() {
        assert_eq!(decode_entities("a & b; c &#xZZ; d"), "a & b; c &#xZZ; d");
        assert_eq!(decode_entities("&#233;t&#xE9;"), "été");
    }
}
