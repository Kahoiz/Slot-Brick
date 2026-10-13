//! Matching ROM file names against the libretro thumbnail collection
//! (thumbnails.libretro.com). Names there are No-Intro names with the characters
//! `&*/:`<>?\|"` replaced by `_`; ROMs on a card are often named differently, so
//! this matches on a normalised title and then picks the best region and release.

use unicode_normalization::UnicodeNormalization;

use crate::Platform;

/// The kinds of picture libretro keeps per game, as its directory names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThumbKind {
    Boxart,
    Title,
    Snap,
}

impl ThumbKind {
    pub const ALL: [ThumbKind; 3] = [ThumbKind::Boxart, ThumbKind::Title, ThumbKind::Snap];

    pub fn dir(self) -> &'static str {
        match self {
            ThumbKind::Boxart => "Named_Boxarts",
            ThumbKind::Title => "Named_Titles",
            ThumbKind::Snap => "Named_Snaps",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThumbKind::Boxart => "BOX ART",
            ThumbKind::Title => "TITLE SCREEN",
            ThumbKind::Snap => "IN-GAME SHOT",
        }
    }

    /// `self` first, then the other kinds, for games missing the preferred one.
    pub fn with_fallbacks(self) -> Vec<ThumbKind> {
        let mut kinds = vec![self];
        kinds.extend(Self::ALL.into_iter().filter(|k| *k != self));
        kinds
    }
}

/// libretro's system directory name, eg "Nintendo - Game Boy Advance".
pub fn thumb_system(platform: Platform) -> String {
    format!("Nintendo - {}", platform.name())
}

/// A name as libretro stores it on disk.
pub fn libretro_name(name: &str) -> String {
    name.chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect()
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
pub fn url_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3 / 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Game names (without ".png") from a thumbnail directory's HTML index.
pub fn parse_listing(html: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("href=\"") {
        rest = &rest[at + 6..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let href = rest[..end].replace("&amp;", "&");
        rest = &rest[end..];
        if href.contains('/') {
            continue;
        }
        let name = url_unescape(&href);
        if let Some(stem) = name.strip_suffix(".png") {
            if !stem.is_empty() {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The title with tags, punctuation, accents, case and "the" removed:
/// "Legend of Zelda, The - Link's Awakening DX (USA, Europe) (SGB Enhanced)"
/// and "the_legend_of_zelda_links_awakening_dx" both give "legendofzeldalinksawakeningdx".
pub fn title_key(name: &str) -> String {
    let mut plain = String::with_capacity(name.len());
    let mut depth = 0u32;
    for c in name.nfd() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '&' if depth == 0 => plain.push_str(" and "),
            '\'' => {}
            c if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !w.eq_ignore_ascii_case("the"))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Every comma-separated tag inside ( ) and [ ].
fn tags(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for c in name.chars() {
        match (c, &mut cur) {
            ('(' | '[', _) => cur = Some(String::new()),
            (')' | ']', Some(t)) => {
                out.extend(t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
                cur = None;
            }
            (c, Some(t)) => t.push(c),
            _ => {}
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    Usa,
    World,
    Europe,
    Japan,
}

fn regions(name: &str) -> Vec<Region> {
    let mut out = Vec::new();
    for tag in tags(name) {
        match tag.as_str() {
            "USA" => out.push(Region::Usa),
            "World" => out.push(Region::World),
            "Europe" => out.push(Region::Europe),
            "Japan" => out.push(Region::Japan),
            // GoodTools: (U), (E), (J), (UE), (JU) ...
            t if !t.is_empty() && t.len() <= 3 && t.chars().all(|c| "UEJW".contains(c)) => {
                for c in t.chars() {
                    out.push(match c {
                        'U' => Region::Usa,
                        'E' => Region::Europe,
                        'J' => Region::Japan,
                        _ => Region::World,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

const UNWANTED: [&str; 9] = [
    "Beta", "Proto", "Demo", "Sample", "Kiosk", "Pirate", "Unl", "Hack", "Aftermarket",
];
/// Re-releases: fine, but the original cartridge is preferred.
const REISSUE: [&str; 4] = ["Virtual Console", "Switch Online", "Alternate", "Promo"];

/// `asked`: the ROM's own tags. A ROM tagged (Hack) or (Beta) wants one of those.
fn score(candidate: &str, wanted: &[Region], asked: &[String]) -> i32 {
    let tags = tags(candidate);
    let mut score = 0;
    let unwanted = |t: &String| {
        UNWANTED
            .iter()
            .any(|u| t.starts_with(u) && !asked.iter().any(|a| a.starts_with(u)))
    };
    if tags.iter().any(unwanted) {
        score -= 100;
    }
    if tags.iter().any(|t| REISSUE.contains(&t.as_str())) {
        score -= 5;
    }
    let have = regions(candidate);
    if !wanted.is_empty() && have.iter().any(|r| wanted.contains(r)) {
        score += 50;
    }
    score += have
        .iter()
        .map(|r| match r {
            Region::Usa => 30,
            Region::World => 25,
            Region::Europe => 20,
            Region::Japan => 10,
        })
        .max()
        .unwrap_or(0);
    score - tags.len() as i32
}

/// Below this many characters a title prefix is too vague to trust ("zelda").
const PREFIX_MIN: usize = 8;

/// The libretro name for a ROM named `stem`, if any of `names` is a good match.
pub fn best_match<'a>(stem: &str, names: &'a [String]) -> Option<&'a str> {
    let exact = libretro_name(stem);
    if let Some(n) = names.iter().find(|n| n.eq_ignore_ascii_case(&exact)) {
        return Some(n);
    }
    let key = title_key(stem);
    if key.is_empty() {
        return None;
    }
    let wanted = regions(stem);
    let asked = tags(stem);
    let pick = |pool: Vec<&'a String>| {
        pool.into_iter()
            .max_by(|a, b| {
                score(a, &wanted, &asked)
                    .cmp(&score(b, &wanted, &asked))
                    .then(b.len().cmp(&a.len()))
                    .then(b.cmp(a))
            })
            .map(String::as_str)
    };
    // A ROM that says it is a hack only gets art made for a hack, never the original's.
    let is_hack = |tags: &[String]| tags.iter().any(|t| t.starts_with("Hack"));
    let hack = is_hack(&asked);
    let keyed: Vec<(&String, String)> = names
        .iter()
        .filter(|n| !hack || is_hack(&tags(n)))
        .map(|n| (n, title_key(n)))
        .collect();
    let same: Vec<&String> = keyed.iter().filter(|(_, k)| *k == key).map(|(n, _)| *n).collect();
    if !same.is_empty() {
        return pick(same);
    }
    if key.len() < PREFIX_MIN {
        return None;
    }
    // "pokemon_emerald" -> "Pokemon - Emerald Version": the shortest longer title wins.
    // Never the other way round, or "golden_sun_2" would get "Golden Sun".
    let near: Vec<(&String, usize)> = keyed
        .iter()
        .filter(|(_, k)| k.starts_with(&key))
        .map(|(n, k)| (*n, k.len() - key.len()))
        .collect();
    let closest = near.iter().map(|(_, d)| *d).min()?;
    pick(near.into_iter().filter(|(_, d)| *d == closest).map(|(n, _)| n).collect())
}
