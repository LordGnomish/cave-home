//! Grandma-friendly, localised music phrases (Charter §6.3, ADR-007).
//!
//! The MPD wire protocol — `OK`, `ACK [50@0]`, `songid`, `playlistinfo` — never
//! reaches the end-user. The Portal, the mobile app and the voice assistant show
//! the phrases built here: "Playing … by …", "Music stopped", "Repeat is on",
//! localised to EN / DE / TR (the Charter §6.3 mandatory languages from M1).

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// Build "Playing &lt;title&gt; by &lt;artist&gt;" in the requested language.
///
/// When the artist is unknown only the title is announced. Both strings are
/// caller-supplied song metadata, never protocol tokens.
#[must_use]
pub fn now_playing(title: &str, artist: Option<&str>, lang: Lang) -> String {
    match (artist, lang) {
        (Some(a), Lang::En) => format!("Playing {title} by {a}"),
        (Some(a), Lang::De) => format!("{title} von {a} wird gespielt"),
        (Some(a), Lang::Tr) => format!("{a} sanatçısından {title} çalınıyor"),
        (None, Lang::En) => format!("Playing {title}"),
        (None, Lang::De) => format!("{title} wird gespielt"),
        (None, Lang::Tr) => format!("{title} çalınıyor"),
    }
}

/// "Music is paused" in the requested language.
#[must_use]
pub const fn paused(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Music is paused",
        Lang::De => "Die Musik ist angehalten",
        Lang::Tr => "Müzik duraklatıldı",
    }
}

/// "Music stopped" in the requested language.
#[must_use]
pub const fn stopped(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Music stopped",
        Lang::De => "Die Musik wurde gestoppt",
        Lang::Tr => "Müzik durduruldu",
    }
}

/// "Nothing is playing" — used when the queue is empty or playback is idle.
#[must_use]
pub const fn nothing_playing(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Nothing is playing",
        Lang::De => "Es wird nichts gespielt",
        Lang::Tr => "Hiçbir şey çalmıyor",
    }
}

/// "Repeat is on" / "Repeat is off".
#[must_use]
pub const fn repeat(on: bool, lang: Lang) -> &'static str {
    match (on, lang) {
        (true, Lang::En) => "Repeat is on",
        (true, Lang::De) => "Wiederholung ist an",
        (true, Lang::Tr) => "Tekrar açık",
        (false, Lang::En) => "Repeat is off",
        (false, Lang::De) => "Wiederholung ist aus",
        (false, Lang::Tr) => "Tekrar kapalı",
    }
}

/// "Shuffle is on" / "Shuffle is off" (the friendly name for `random`).
#[must_use]
pub const fn shuffle(on: bool, lang: Lang) -> &'static str {
    match (on, lang) {
        (true, Lang::En) => "Shuffle is on",
        (true, Lang::De) => "Zufallswiedergabe ist an",
        (true, Lang::Tr) => "Karışık çalma açık",
        (false, Lang::En) => "Shuffle is off",
        (false, Lang::De) => "Zufallswiedergabe ist aus",
        (false, Lang::Tr) => "Karışık çalma kapalı",
    }
}

/// "Volume is &lt;n&gt;%" in the requested language.
#[must_use]
pub fn volume(level: u8, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Volume is {level}%"),
        Lang::De => format!("Lautstärke ist {level}%"),
        Lang::Tr => format!("Ses seviyesi %{level}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_playing_with_and_without_artist() {
        assert_eq!(
            now_playing("Yesterday", Some("The Beatles"), Lang::En),
            "Playing Yesterday by The Beatles"
        );
        assert_eq!(now_playing("Yesterday", None, Lang::En), "Playing Yesterday");
        assert_eq!(
            now_playing("Yesterday", Some("The Beatles"), Lang::Tr),
            "The Beatles sanatçısından Yesterday çalınıyor"
        );
    }

    #[test]
    fn toggles_and_volume_localise() {
        assert_eq!(repeat(true, Lang::De), "Wiederholung ist an");
        assert_eq!(shuffle(false, Lang::Tr), "Karışık çalma kapalı");
        assert_eq!(volume(40, Lang::En), "Volume is 40%");
        assert_eq!(volume(40, Lang::Tr), "Ses seviyesi %40");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/cluster terms.
        const BANNED: &[&str] = &[
            "MPD", "ACK", "songid", "Pos:", "playlistinfo", "MQTT", "songpos",
            "tracklist", "Mopidy", "entity_id", "pod", "kubelet", "TCP",
        ];
        let mut texts: Vec<String> = Vec::new();
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            texts.push(now_playing("Song", Some("Artist"), lang));
            texts.push(now_playing("Song", None, lang));
            texts.push(paused(lang).to_owned());
            texts.push(stopped(lang).to_owned());
            texts.push(nothing_playing(lang).to_owned());
            texts.push(repeat(true, lang).to_owned());
            texts.push(repeat(false, lang).to_owned());
            texts.push(shuffle(true, lang).to_owned());
            texts.push(shuffle(false, lang).to_owned());
            texts.push(volume(55, lang));
        }
        for text in &texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "phrase leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
