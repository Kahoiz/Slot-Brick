//! Fetches real cartridge label scans for games with no label, over HTTPS, from the slot
//! art set behind Cart Studio (art.slot-cfw.fyi, scans from ScreenScraper), found by the
//! ROM's CRC32. A label the user made is never replaced: only carts without one are
//! looked up. Slot is built around the cartridge, so no other kind of art is fetched.
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slot_store::{atomic_write, best_match, scan, Cart, Platform};

/// An origin: `https://host`, or `http://host:port` for the tests' local server.
const SLOT_ART: &str = "https://art.slot-cfw.fyi";
const LISTING_MAX: usize = 8 << 20;
const IMAGE_MAX: usize = 8 << 20;
/// The index changes rarely; a week-old copy saves a megabyte per run.
const LISTING_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const CACHE_DIR: &str = "System/Cache/Thumbs";
const LABEL_INDEX: &str = "slot-art-labels.txt";
/// ROM checksums, so a slow card is read once per ROM rather than once per run.
const CRC_CACHE: &str = "System/Cache/crc32.txt";
/// No-Intro's games (platform, CRC32, serial, name), shipped on the card: built by
/// brick/make-cart-art-games.py, so the multi-megabyte DATs are never downloaded here.
const GAMES: &str = "System/cart-art-games.txt";
const REPORT: &str = "Logs/cart-art.txt";

/// A label written to (or, with None, deleted from) the card, for the running library to pick up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    pub platform: Platform,
    pub stem: String,
    pub label: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Progress {
    Step { done: usize, total: usize, name: String },
    Got(Fetched),
    Done(Result<Summary, String>),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub found: usize,
    pub missing: usize,
    pub failed: usize,
}

pub struct ArtJob {
    rx: mpsc::Receiver<Progress>,
    cancel: Arc<AtomicBool>,
}

impl ArtJob {
    pub fn start(root: PathBuf) -> Self {
        Self::start_at(root, SLOT_ART.into())
    }

    /// `server`: the label server's origin; tests point it at a local server.
    fn start_at(root: PathBuf, server: String) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        thread::spawn(move || {
            let done = run(&root, &server, &token, &tx);
            let _ = tx.send(Progress::Done(done));
        });
        ArtJob { rx, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn try_recv(&self) -> Result<Progress, mpsc::TryRecvError> {
        self.rx.try_recv()
    }
}

impl Drop for ArtJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Everything fetched or computed once per run.
struct Sources<'a> {
    root: &'a Path,
    server: &'a str,
    labels: Option<HashMap<u32, String>>,
    crcs: Crcs,
    games: Games,
}

fn run(
    root: &Path,
    server: &str,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<Progress>,
) -> Result<Summary, String> {
    let carts: Vec<Cart> = scan(root)
        .map_err(|e| format!("Cannot read games: {e}"))?
        .into_iter()
        .filter(|c| c.label.is_none())
        .collect();
    let mut summary = Summary::default();
    let mut report = String::new();
    let mut src = Sources {
        root,
        server,
        labels: None,
        crcs: Crcs::load(root),
        games: Games::load(root),
    };
    let result = (|| {
        for (at, cart) in carts.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".to_string());
            }
            let _ = tx.send(Progress::Step {
                done: at,
                total: carts.len(),
                name: cart.stem.clone(),
            });
            // No label scan (homebrew, most hacks): the shelf's file-name label stays.
            let outcome = fetch_label(cart, &mut src)?;
            let place = format!("{}/{}", cart.platform.dir_name(), cart.stem);
            match outcome {
                Outcome::Saved(from, label) => {
                    summary.found += 1;
                    report.push_str(&format!("found     {place} <- {from}\n"));
                    let _ = tx.send(Progress::Got(Fetched {
                        platform: cart.platform,
                        stem: cart.stem.clone(),
                        label: Some(label),
                    }));
                }
                Outcome::NoMatch => {
                    summary.missing += 1;
                    report.push_str(&format!("no match  {place}\n"));
                }
                Outcome::Failed(why) => {
                    summary.failed += 1;
                    report.push_str(&format!("failed    {place}: {why}\n"));
                }
            }
        }
        Ok(())
    })();
    // Checksums read before a cancel or a network failure are kept for next time.
    src.crcs.save(root);
    result?;
    eprintln!(
        "slot: cart art: {} found, {} no match, {} failed",
        summary.found, summary.missing, summary.failed
    );
    if let Err(e) = std::fs::write(root.join(REPORT), report) {
        eprintln!("slot: cart art: {REPORT}: {e}");
    }
    Ok(summary)
}

enum Outcome {
    /// Where the art came from, for the report, and the label written.
    Saved(String, PathBuf),
    NoMatch,
    Failed(String),
}

/// Platforms whose game lists may hold a cart: GB and GBC games are often
/// filed under the other one (Tetris DX is a GBC game many keep with GB games).
fn platforms_for(platform: Platform) -> &'static [Platform] {
    match platform {
        Platform::Gba => &[Platform::Gba],
        Platform::Gb => &[Platform::Gb, Platform::Gbc],
        Platform::Gbc => &[Platform::Gbc, Platform::Gb],
    }
}

/// A real label scan. Found by the ROM's CRC32, or failing that (a trimmed or patched
/// ROM, a hack) by the GBA game code in its header, then by its file name.
fn fetch_label(cart: &Cart, src: &mut Sources) -> Result<Outcome, String> {
    let crc = match src.crcs.of(src.root, &cart.rom) {
        Ok(crc) => crc,
        Err(e) => return Ok(Outcome::Failed(format!("cannot read ROM: {e}"))),
    };
    if src.labels.is_none() {
        src.labels = Some(label_index(src.root, src.server)?);
    }
    let mut cart = cart.clone();
    cart.read_header();
    let labels = src.labels.as_ref().expect("just loaded");
    let Some((path, how)) = src
        .games
        .candidates(&cart, crc)
        .into_iter()
        .find_map(|(crc, how)| Some((labels.get(&crc)?.clone(), how)))
    else {
        return Ok(Outcome::NoMatch);
    };
    let path = &path;
    let png = match get(src.server, &format!("/{path}"), IMAGE_MAX) {
        Ok(Some(png)) if is_png(&png) => png,
        Ok(Some(_)) => return Ok(Outcome::Failed(format!("{path}: not a PNG"))),
        Ok(None) => return Ok(Outcome::NoMatch),
        Err(e) => return Ok(Outcome::Failed(e)),
    };
    Ok(save_label(src.root, &cart, &png, format!("label scan {path} ({how})")))
}

struct Game {
    platform: Platform,
    crc: u32,
    serial: String,
    name: String,
}

/// The shipped No-Intro list, to turn a game code or a name into CRCs the label server knows.
#[derive(Default)]
struct Games {
    list: Vec<Game>,
    names: HashMap<Platform, Vec<String>>,
}

impl Games {
    fn load(root: &Path) -> Self {
        let text = std::fs::read_to_string(root.join(GAMES)).unwrap_or_default();
        let list: Vec<Game> = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut f = l.splitn(4, '\t');
                let platform = match f.next()? {
                    "GBA" => Platform::Gba,
                    "GB" => Platform::Gb,
                    "GBC" => Platform::Gbc,
                    _ => return None,
                };
                let crc = u32::from_str_radix(f.next()?, 16).ok()?;
                let serial = f.next()?.to_string();
                Some(Game { platform, crc, serial, name: f.next()?.to_string() })
            })
            .collect();
        let mut names: HashMap<Platform, Vec<String>> = HashMap::new();
        for g in &list {
            names.entry(g.platform).or_default().push(slot_store::libretro_name(&g.name));
        }
        Games { list, names }
    }

    /// CRCs worth looking up for `cart` (its header read), best first, each with how it was found.
    fn candidates(&self, cart: &Cart, crc: u32) -> Vec<(u32, String)> {
        let mut out = vec![(crc, "checksum".to_string())];
        let platforms = platforms_for(cart.platform);
        let code = cart.code.trim();
        // A trimmed or patched ROM keeps its game code, but so does a hack: the code only
        // counts when the file name names that game too, so a hack never wears its base
        // game's label and stays told apart by its own (file name) label.
        let named = |g: &Game| g.serial == code || g.serial.get(..3) == code.get(..3);
        let group: Vec<String> = self
            .list
            .iter()
            .filter(|g| g.platform == Platform::Gba && named(g))
            .map(|g| slot_store::libretro_name(&g.name))
            .collect();
        let code_ok = cart.platform == Platform::Gba
            && code.len() == 4
            && code.bytes().all(|b| b.is_ascii_alphanumeric())
            && best_match(&cart.stem, &group).is_some();
        if code_ok {
            // The exact release first, then the same game from other regions (BZ4E -> BZ4P).
            for same_region in [true, false] {
                for g in self.list.iter().filter(|g| g.platform == Platform::Gba) {
                    let hit = if same_region { g.serial == code } else { g.serial.get(..3) == code.get(..3) };
                    if hit {
                        out.push((g.crc, format!("game code {code}")));
                    }
                }
            }
        }
        for &platform in platforms {
            let Some(names) = self.names.get(&platform) else { continue };
            let Some(name) = best_match(&cart.stem, names) else { continue };
            let key = slot_store::title_key(name);
            let mut same: Vec<&Game> = self
                .list
                .iter()
                .filter(|g| g.platform == platform && slot_store::title_key(&g.name) == key)
                .collect();
            // The matched release first, then its other regions and revisions.
            same.sort_by_key(|g| slot_store::libretro_name(&g.name) != name);
            out.extend(same.into_iter().map(|g| (g.crc, format!("name {name}"))));
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|(crc, _)| seen.insert(*crc));
        out
    }
}

fn save_label(root: &Path, cart: &Cart, png: &[u8], from: String) -> Outcome {
    let label = root
        .join("Labels")
        .join(cart.platform.dir_name())
        .join(format!("{}.png", cart.stem));
    if label.exists() {
        return Outcome::Failed("a label appeared meanwhile; kept it".into());
    }
    if let Some(dir) = label.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match atomic_write(&label, png) {
        Ok(()) => Outcome::Saved(from, label),
        Err(e) => Outcome::Failed(format!("cannot write label: {e}")),
    }
}

/// CRC32 -> path of its label scan, from the art set's index.json (cached on the card).
fn label_index(root: &Path, server: &str) -> Result<HashMap<u32, String>, String> {
    let cache = root.join(CACHE_DIR).join(LABEL_INDEX);
    let text = match read_fresh(&cache) {
        Some(lines) => lines.join("\n"),
        None => {
            let json = get(server, "/index.json", LISTING_MAX)
                .map_err(|e| format!("Can't reach the label server: {e}"))?
                .ok_or("The label server has no index")?;
            let pairs = parse_label_index(&String::from_utf8_lossy(&json));
            let text: Vec<String> = pairs.iter().map(|(crc, path)| format!("{crc:08X} {path}")).collect();
            if let Some(dir) = cache.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = atomic_write(&cache, text.join("\n").as_bytes());
            text.join("\n")
        }
    };
    Ok(text
        .lines()
        .filter_map(|l| {
            let (crc, path) = l.split_once(' ')?;
            Some((u32::from_str_radix(crc, 16).ok()?, label_path(path)?.to_string()))
        })
        .collect())
}

/// Every "support-texture" (label scan) path in index.json, keyed by the CRC in its name.
/// Only paths of the exact form `<GB|GBC|GBA>/support-texture/<CRC32>.png` are taken,
/// so the index can never send a request anywhere else.
fn parse_label_index(json: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(at) = rest.find("\"support-texture\"") {
        rest = &rest[at + 17..];
        let Some(open) = rest.find('"') else { break };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        if let Some(path) = label_path(&after[..close]) {
            let crc = &path[path.len() - 12..path.len() - 4];
            if let Ok(crc) = u32::from_str_radix(crc, 16) {
                out.push((crc, path.to_string()));
            }
        }
        rest = &after[close..];
    }
    out
}

fn label_path(path: &str) -> Option<&str> {
    let (system, rest) = path.split_once('/')?;
    let file = rest.strip_prefix("support-texture/")?;
    let crc = file.strip_suffix(".png")?;
    let ok = ["GB", "GBC", "GBA"].contains(&system)
        && crc.len() == 8
        && crc.bytes().all(|b| b.is_ascii_hexdigit());
    ok.then_some(path)
}

/// ROM checksums by path, kept with each file's size and mtime.
struct Crcs {
    known: HashMap<String, (u64, u64, u32)>,
    changed: bool,
}

impl Crcs {
    fn load(root: &Path) -> Self {
        let text = std::fs::read_to_string(root.join(CRC_CACHE)).unwrap_or_default();
        let known = text
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(4, '\t');
                let crc = u32::from_str_radix(f.next()?, 16).ok()?;
                let size = f.next()?.parse().ok()?;
                let mtime = f.next()?.parse().ok()?;
                Some((f.next()?.to_string(), (size, mtime, crc)))
            })
            .collect();
        Crcs {
            known,
            changed: false,
        }
    }

    fn of(&mut self, root: &Path, rom: &Path) -> std::io::Result<u32> {
        let meta = std::fs::metadata(rom)?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let key = rom.strip_prefix(root).unwrap_or(rom).to_string_lossy().into_owned();
        if let Some(&(size, when, crc)) = self.known.get(&key) {
            if (size, when) == (meta.len(), mtime) {
                return Ok(crc);
            }
        }
        let crc = crc32_file(rom)?;
        self.known.insert(key, (meta.len(), mtime, crc));
        self.changed = true;
        Ok(crc)
    }

    fn save(&self, root: &Path) {
        if !self.changed {
            return;
        }
        let mut lines: Vec<String> = self
            .known
            .iter()
            .filter(|(k, _)| !k.contains(['\n', '\t']))
            .map(|(k, (size, mtime, crc))| format!("{crc:08X}\t{size}\t{mtime}\t{k}"))
            .collect();
        lines.sort();
        let path = root.join(CRC_CACHE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = atomic_write(&path, lines.join("\n").as_bytes()) {
            eprintln!("slot: cart art: {CRC_CACHE}: {e}");
        }
    }
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// The CRC32 No-Intro and the art set index ROMs by.
fn crc32(crc: u32, bytes: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in bytes {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

fn crc32_file(path: &Path) -> std::io::Result<u32> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 256 << 10];
    let mut crc = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(crc);
        }
        crc = crc32(crc, &buf[..n]);
    }
}

fn is_png(bytes: &[u8]) -> bool {
    let mut dec = png::Decoder::new(bytes);
    dec.set_limits(png::Limits { bytes: 64 << 20 });
    dec.read_info().is_ok_and(|r| {
        let info = r.info();
        info.width > 0 && info.height > 0 && info.width <= 4096 && info.height <= 4096
    })
}

fn read_fresh(path: &Path) -> Option<Vec<String>> {
    let age = SystemTime::now()
        .duration_since(std::fs::metadata(path).ok()?.modified().ok()?)
        .unwrap_or_default();
    if age > LISTING_TTL {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let names: Vec<String> = text.lines().filter(|l| !l.is_empty()).map(String::from).collect();
    (!names.is_empty()).then_some(names)
}

/// GET over HTTP/1.0 (so no chunked bodies) from an origin. Ok(None) is a 404.
fn get(server: &str, path: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let mut path = path.to_string();
    for _ in 0..3 {
        match request(server, &path, limit)? {
            Reply::Body(body) => return Ok(Some(body)),
            Reply::NotFound => return Ok(None),
            Reply::Moved(to) => path = to,
        }
    }
    Err("too many redirects".into())
}

enum Reply {
    Body(Vec<u8>),
    NotFound,
    Moved(String),
}

fn request(origin: &str, path: &str, limit: usize) -> Result<Reply, String> {
    let (tls, server, _) = crate::net::split_url(origin).ok_or_else(|| format!("bad server {origin}"))?;
    let reply = crate::net::send(&crate::net::Request {
        tls,
        server: &server,
        method: "GET",
        path,
        form: None,
        limit,
    })?;
    reply_of(&reply, origin)
}

/// A redirect is only followed on the same origin, so never from HTTPS down to HTTP.
fn reply_of(reply: &crate::net::Response, origin: &str) -> Result<Reply, String> {
    match reply.status {
        200 => Ok(Reply::Body(reply.body.clone())),
        404 => Ok(Reply::NotFound),
        301 | 302 | 303 | 307 | 308 => {
            let to = reply.header("location").ok_or("redirect without a location")?;
            match to.strip_prefix(origin) {
                Some(rest) if rest.starts_with('/') => Ok(Reply::Moved(rest.to_string())),
                _ if to.starts_with('/') => Ok(Reply::Moved(to)),
                _ => Err(format!("server moved to {to}, which is not supported")),
            }
        }
        status => Err(format!("server said {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// One-shot-per-connection HTTP server answering from a fixed table.
    fn serve(routes: Vec<(String, u16, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut raw = Vec::new();
                let mut buf = [0u8; 1024];
                while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    match conn.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&raw).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let (status, body) = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((404, Vec::new()));
                let _ = write!(conn, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n", body.len());
                let _ = conn.write_all(&body);
            }
        });
        format!("http://{addr}")
    }

    fn png_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 2, 2);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[200; 16]).unwrap();
        out
    }

    fn rom(root: &Path, rel: &str) {
        let path = root.join("Games").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; 0x200]).unwrap();
    }

    fn drain(job: &ArtJob) -> (Vec<Fetched>, Result<Summary, String>) {
        let mut got = Vec::new();
        loop {
            match job.rx.recv_timeout(Duration::from_secs(20)).expect("job finished") {
                Progress::Got(f) => got.push(f),
                Progress::Done(r) => return (got, r),
                Progress::Step { .. } => {}
            }
        }
    }

    #[test]
    fn an_unreachable_server_stops_with_a_message() {
        let d = tempfile::tempdir().unwrap();
        let err = label_index(d.path(), "http://127.0.0.1:1").unwrap_err();
        assert!(err.starts_with("Can't reach the label server"), "{err}");
    }

    #[test]
    fn redirects_on_the_same_host_are_followed_and_others_refused() {
        let reply = |raw: &[u8]| reply_of(&crate::net::parse(raw, 100).unwrap(), "https://h");
        let moved = b"HTTP/1.1 301 Moved\r\nLocation: https://h/new/\r\n\r\n";
        assert!(matches!(reply(moved), Ok(Reply::Moved(p)) if p == "/new/"));
        let down = b"HTTP/1.1 301 Moved\r\nLocation: http://h/new/\r\n\r\n";
        assert!(reply(down).is_err());
        let away = b"HTTP/1.1 301 Moved\r\nLocation: https://hx/new/\r\n\r\n";
        assert!(reply(away).is_err());
    }

    /// The real label server: `SLOT_LIVE_ART=1 ... live_label_server -- --ignored`, and
    /// SLOT_ART_PREVIEW=<dir> to render one GBA and one GB cart with a real label.
    #[test]
    #[ignore = "needs internet"]
    fn live_label_server() {
        if std::env::var_os("SLOT_LIVE_ART").is_none() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let index = label_index(root, SLOT_ART).unwrap();
        assert!(index.len() > 5000, "{} label scans", index.len());
        for system in ["GBA", "GB"] {
            let (crc, path) = index.iter().filter(|(_, p)| p.starts_with(&format!("{system}/"))).min().unwrap();
            let png = get(SLOT_ART, &format!("/{path}"), IMAGE_MAX).unwrap().unwrap();
            assert!(is_png(&png), "{path}");
            if std::env::var_os("SLOT_ART_PREVIEW").is_none() {
                continue;
            }
            rom(root, &format!("{system}/{crc:08X}.{}", system.to_lowercase()));
            std::fs::create_dir_all(root.join("Labels").join(system)).unwrap();
            std::fs::write(root.join(format!("Labels/{system}/{crc:08X}.png")), &png).unwrap();
        }
        if let Some(out) = std::env::var_os("SLOT_ART_PREVIEW") {
            for mut cart in scan(root).unwrap().into_iter().filter(|c| c.label.is_some()) {
                cart.read_header();
                let face = slot_ui::cart_face_with(&cart, crate::label_cache::label_art(root, &cart));
                let path = Path::new(&out).join(format!("label-{}.png", cart.platform.dir_name()));
                let mut e = png::Encoder::new(std::fs::File::create(path).unwrap(), face.w, face.h);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&face.rgba).unwrap();
            }
        }
    }

    fn games(root: &Path, lines: &[&str]) -> Games {
        std::fs::create_dir_all(root.join("System")).unwrap();
        std::fs::write(root.join(GAMES), format!("# test\n{}\n", lines.join("\n"))).unwrap();
        Games::load(root)
    }

    fn gba_rom(root: &Path, name: &str, code: &str) -> Cart {
        let mut rom = vec![0u8; 0x200];
        rom[0xa0..0xa0 + 12].copy_from_slice(b"FINAL FANTAS");
        rom[0xac..0xb0].copy_from_slice(code.as_bytes());
        let path = root.join("Games/GBA").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rom).unwrap();
        let mut cart = scan(root).unwrap().into_iter().find(|c| c.rom == path).unwrap();
        cart.read_header();
        cart
    }

    #[test]
    fn a_trimmed_gba_rom_is_found_by_its_game_code_but_a_hack_is_not() {
        let d = tempfile::tempdir().unwrap();
        let g = games(d.path(), &[
            "GBA\t27F2BC67\tBZ4P\tFinal Fantasy IV Advance (Europe) (En,Fr,De,Es,It)",
            "GBA\tFEEEBDE4\tBZ4E\tFinal Fantasy IV Advance (USA)",
            "GBA\tDD88761C\tBPRE\tPokemon - FireRed Version (USA, Europe)",
        ]);
        let ff = gba_rom(d.path(), "Final Fantasy IV Advance (trimmed).gba", "BZ4E");
        let crcs: Vec<u32> = g.candidates(&ff, 0x1234).into_iter().map(|(c, _)| c).collect();
        assert_eq!(crcs, vec![0x1234, 0xFEEE_BDE4, 0x27F2_BC67], "own, same region, other regions");
        let hack = gba_rom(d.path(), "Pokemon Radical Red.gba", "BPRE");
        assert_eq!(g.candidates(&hack, 0x5678), vec![(0x5678, "checksum".to_string())], "a hack keeps its own label");
        let vague = gba_rom(d.path(), "ff4.gba", "BZ4E");
        assert_eq!(g.candidates(&vague, 9).len(), 1, "a code alone is not enough");
    }

    #[test]
    fn a_game_boy_rom_falls_back_to_its_name_and_other_releases() {
        let d = tempfile::tempdir().unwrap();
        let g = games(d.path(), &[
            "GB\t9F7FDD53\t-\tPokemon - Red Version (USA, Europe) (SGB Enhanced)",
            "GB\tAAAAAAAA\t-\tPokemon - Red Version (Germany) (SGB Enhanced)",
            "GBC\t46DF91AD\t-\tTetris DX (World) (SGB Enhanced) (GB Compatible)",
        ]);
        rom(d.path(), "GB/Pokemon Red (G).gb");
        rom(d.path(), "GB/Tetris DX.gb");
        let carts = scan(d.path()).unwrap();
        let red = carts.iter().find(|c| c.stem.starts_with("Pokemon")).unwrap();
        let crcs: Vec<u32> = g.candidates(red, 1).into_iter().map(|(c, _)| c).collect();
        assert_eq!(crcs, vec![1, 0xAAAA_AAAA, 0x9F7F_DD53], "the matched release, then its siblings");
        let tetris = carts.iter().find(|c| c.stem == "Tetris DX").unwrap();
        assert!(g.candidates(tetris, 1).iter().any(|(c, _)| *c == 0x46DF_91AD), "GBC list searched for a GB shelf");
    }

    #[test]
    fn a_label_is_found_for_a_rom_whose_checksum_is_unknown() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        games(root, &["GBA\tFEEEBDE4\tBZ4E\tFinal Fantasy IV Advance (USA)"]);
        gba_rom(root, "Final Fantasy IV Advance (trimmed).gba", "BZ4E");
        std::fs::create_dir_all(root.join("Logs")).unwrap();
        let path = "GBA/support-texture/FEEEBDE4.png";
        let server = serve(vec![
            ("/index.json".into(), 200, format!(r#"{{"FEEEBDE4": {{"support-texture": "{path}"}}}}"#).into_bytes()),
            (format!("/{path}"), 200, png_bytes()),
        ]);
        let (_, done) = drain(&ArtJob::start_at(root.to_path_buf(), server));
        assert_eq!(done, Ok(Summary { found: 1, missing: 0, failed: 0 }));
        let report = std::fs::read_to_string(root.join(REPORT)).unwrap();
        assert!(report.contains("(game code BZ4E)"), "{report}");
    }

    #[test]
    fn the_shipped_game_list_loads() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../brick/card/Slot");
        let g = Games::load(&shipped);
        assert!(g.list.len() > 7000, "{} games", g.list.len());
        assert!(g.list.iter().any(|x| x.serial == "BPRE" && x.name.starts_with("Pokemon - FireRed")));
    }

    /// ROMs whose checksums the server cannot know, against the real server and the shipped
    /// game list: `SLOT_LIVE_ART=1 ... live_label_fallbacks -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs internet"]
    fn live_label_fallbacks() {
        if std::env::var_os("SLOT_LIVE_ART").is_none() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("System")).unwrap();
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../brick/card/Slot").join(GAMES);
        std::fs::copy(shipped, root.join(GAMES)).unwrap();
        std::fs::create_dir_all(root.join("Logs")).unwrap();
        gba_rom(root, "Final Fantasy IV Advance (trimmed).gba", "BZ4E");
        gba_rom(root, "Pokemon Radical Red.gba", "BPRE");
        gba_rom(root, "Pokemon Glazed.gba", "BPEE");
        rom(root, "GB/Pokemon - Red Version (U) [!].gb");
        rom(root, "GBA/My Homebrew.gba");
        let (_, done) = drain(&ArtJob::start(root.to_path_buf()));
        let report = std::fs::read_to_string(root.join(REPORT)).unwrap_or_default();
        eprintln!("{report}");
        assert_eq!(done, Ok(Summary { found: 2, missing: 3, failed: 0 }));
        assert!(report.contains("no match  GBA/Pokemon Radical Red"), "a hack keeps its own label");
        assert!(report.contains("no match  GBA/Pokemon Glazed"), "a hack keeps its own label");
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xCBF4_3926, "streams in pieces");
    }

    #[test]
    fn the_label_index_only_yields_label_scan_paths() {
        let json = r#"{"000206C0": {"look": {"ground": "3067a3"}, "support-texture": "GBA/support-texture/000206C0.png", "wheel": "GBA/wheel/000206C0.png"},
            "00202DFA": {"wheel": "GB/wheel/00202DFA.png"},
            "DEADBEEF": {"support-texture": "../../etc/passwd"},
            "0006612D": {"support-texture":"GB/support-texture/0006612D.png"}}"#;
        assert_eq!(
            parse_label_index(json),
            vec![
                (0x0002_06C0, "GBA/support-texture/000206C0.png".to_string()),
                (0x0006_612D, "GB/support-texture/0006612D.png".to_string()),
            ]
        );
    }

    #[test]
    fn real_labels_are_found_by_checksum_whatever_the_rom_is_called() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        rom(root, "GBA/my weird name.gba");
        rom(root, "GB/homebrew.gb");
        std::fs::write(root.join("Games/GB/homebrew.gb"), b"something else").unwrap();
        std::fs::create_dir_all(root.join("Logs")).unwrap();
        let crc = crc32(0, &[0u8; 0x200]);
        let path = format!("GBA/support-texture/{crc:08X}.png");
        let index = format!(r#"{{"{crc:08X}": {{"support-texture": "{path}"}}}}"#);
        let server = serve(vec![
            ("/index.json".into(), 200, index.into_bytes()),
            (format!("/{path}"), 200, png_bytes()),
        ]);
        let (got, done) = drain(&ArtJob::start_at(root.to_path_buf(), server.clone()));
        assert_eq!(done, Ok(Summary { found: 1, missing: 1, failed: 0 }));
        assert_eq!(got[0].stem, "my weird name");
        assert!(is_png(&std::fs::read(root.join("Labels/GBA/my weird name.png")).unwrap()));
        let cached = std::fs::read_to_string(root.join(CRC_CACHE)).unwrap();
        assert!(cached.contains(&format!("{crc:08X}	512	")), "{cached}");
        assert!(cached.contains("Games/GBA/my weird name.gba"), "{cached}");
        // Second run: the index comes from the card and nothing is left to fetch.
        let (_, again) = drain(&ArtJob::start_at(root.to_path_buf(), server));
        assert_eq!(again, Ok(Summary { found: 0, missing: 1, failed: 0 }));
    }

    #[test]
    fn a_cached_checksum_is_used_until_the_rom_changes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        rom(root, "GBA/a.gba");
        let rom_path = root.join("Games/GBA/a.gba");
        let mut crcs = Crcs::load(root);
        let real = crcs.of(root, &rom_path).unwrap();
        crcs.save(root);
        let mut text = std::fs::read_to_string(root.join(CRC_CACHE)).unwrap();
        text = text.replacen(&format!("{real:08X}"), "12345678", 1);
        std::fs::write(root.join(CRC_CACHE), text).unwrap();
        assert_eq!(Crcs::load(root).of(root, &rom_path).unwrap(), 0x1234_5678, "trusted while size and mtime match");
        std::fs::write(&rom_path, b"patched").unwrap();
        assert_eq!(Crcs::load(root).of(root, &rom_path).unwrap(), crc32(0, b"patched"));
    }

    #[test]
    fn a_web_page_is_not_saved_as_a_label() {
        assert!(!is_png(b"<html>not found</html>"));
        assert!(is_png(&png_bytes()));
    }
}
