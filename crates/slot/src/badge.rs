//! RetroAchievements badge images, cached on the card so an unlock's toast can show its
//! badge straight away: a game's badges are fetched in the background when it loads.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::net;

const DIR: &str = "System/Cache/Badges";
/// Badges are 64x64 PNGs of a few kilobytes.
const BADGE_MAX: usize = 1 << 20;

/// Where the badge at `url` is (or will be) cached; None for a URL that is not a plain
/// https PNG on a RetroAchievements host with a simple file name, which is never fetched.
pub fn path(root: &Path, url: &str) -> Option<PathBuf> {
    let (tls, server, tail) = net::split_url(url)?;
    let host = net::host_of(&server);
    if !tls || !(host == "retroachievements.org" || host.ends_with(".retroachievements.org")) {
        return None;
    }
    let name = tail.rsplit('/').next()?;
    let ok = name.len() <= 64
        && name.ends_with(".png")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && !name.starts_with('.');
    ok.then(|| root.join(DIR).join(name))
}

/// Fetches whichever of `urls` are not cached yet, one at a time, in the background.
pub fn prefetch(root: &Path, urls: Vec<String>) {
    static FETCHING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let wanted: Vec<(String, PathBuf)> = {
        let mut busy = FETCHING.lock().unwrap_or_else(|e| e.into_inner());
        let busy = busy.get_or_insert_with(HashSet::new);
        urls.into_iter()
            .filter_map(|u| Some((path(root, &u)?, u)))
            .filter(|(p, u)| !p.exists() && busy.insert(u.clone()))
            .map(|(p, u)| (u, p))
            .collect()
    };
    if wanted.is_empty() {
        return;
    }
    let urls: Vec<String> = wanted.iter().map(|(u, _)| u.clone()).collect();
    let spawned = std::thread::Builder::new().name("slot-badges".into()).spawn(move || {
        let mut fetched = 0;
        for (url, path) in &wanted {
            match fetch(url, path) {
                Ok(()) => fetched += 1,
                Err(e) => eprintln!("slot: badge {url}: {e}"),
            }
            FETCHING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).remove(url);
        }
        eprintln!("slot: badges: {fetched} of {} fetched", wanted.len());
    });
    if spawned.is_err() {
        let mut busy = FETCHING.lock().unwrap_or_else(|e| e.into_inner());
        for url in &urls {
            busy.get_or_insert_with(HashSet::new).remove(url);
        }
    }
}

fn fetch(url: &str, path: &Path) -> Result<(), String> {
    let (_, server, tail) = net::split_url(url).ok_or("bad URL")?;
    let reply = net::send(&net::Request {
        tls: true,
        server: &server,
        method: "GET",
        path: &tail,
        form: None,
        limit: BADGE_MAX,
    })?;
    if reply.status != 200 {
        return Err(format!("server said {}", reply.status));
    }
    if !reply.body.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("not a PNG".into());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    slot_store::atomic_write(path, &reply.body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_png_names_map_into_the_cache() {
        let root = Path::new("/r");
        assert_eq!(
            path(root, "https://media.retroachievements.org/Badge/12345.png"),
            Some(PathBuf::from("/r/System/Cache/Badges/12345.png"))
        );
        assert_eq!(path(root, "https://media.retroachievements.org/Badge/12345_lock.png").is_some(), true);
        let ra = "https://media.retroachievements.org/Badge/";
        for bad in [
            "http://media.retroachievements.org/Badge/1.png".to_string(),
            "https://evil.example/Badge/1.png".into(),
            "https://retroachievements.org.evil.example/1.png".into(),
            format!("{ra}..png"),
            format!("{ra}a b.png"),
            format!("{ra}1.gif"),
            format!("{ra}.png"),
            String::new(),
        ] {
            assert_eq!(path(root, &bad), None, "{bad}");
        }
    }

    /// `SLOT_LIVE_NET=1 ... live_badge -- --ignored`: a real badge download.
    #[test]
    #[ignore = "needs internet"]
    fn live_badge() {
        if std::env::var_os("SLOT_LIVE_NET").is_none() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let url = "https://media.retroachievements.org/Badge/00000.png";
        let p = path(d.path(), url).unwrap();
        fetch(url, &p).unwrap();
        assert!(std::fs::read(&p).unwrap().len() > 100);
    }
}
