//! Fetches cartridge art for games with no label from the libretro thumbnail
//! server, over plain HTTP so no TLS stack is needed on the device. A label the
//! user made is never replaced: only carts without one are looked up.
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime};

use slot_store::{
    atomic_write, best_match, parse_listing, scan, thumb_system, url_escape, Cart, Platform,
    ThumbKind,
};

const SERVER: &str = "thumbnails.libretro.com:80";
const LISTING_MAX: usize = 8 << 20;
const IMAGE_MAX: usize = 8 << 20;
/// Listings change rarely; a week-old copy saves a megabyte per system per run.
const LISTING_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const CACHE_DIR: &str = "System/Cache/Thumbs";
const REPORT: &str = "Logs/cart-art.txt";

/// A label written to the card, for the running library to pick up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    pub platform: Platform,
    pub stem: String,
    pub label: PathBuf,
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
    pub fn start(root: PathBuf, kind: ThumbKind) -> Self {
        Self::start_at(root, kind, SERVER.into())
    }

    fn start_at(root: PathBuf, kind: ThumbKind, server: String) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        thread::spawn(move || {
            let done = run(&root, &server, kind, &token, &tx);
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

fn run(
    root: &Path,
    server: &str,
    kind: ThumbKind,
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
    let mut lists = Listings::new(root, server);
    for (at, cart) in carts.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let _ = tx.send(Progress::Step {
            done: at,
            total: carts.len(),
            name: cart.stem.clone(),
        });
        match fetch_one(root, server, cart, kind, &mut lists)? {
            Outcome::Saved(name, used, label) => {
                summary.found += 1;
                report.push_str(&format!("found     {}/{} <- {} ({})\n", cart.platform.dir_name(), cart.stem, name, used.dir()));
                let _ = tx.send(Progress::Got(Fetched {
                    platform: cart.platform,
                    stem: cart.stem.clone(),
                    label,
                }));
            }
            Outcome::NoMatch => {
                summary.missing += 1;
                report.push_str(&format!("no match  {}/{}\n", cart.platform.dir_name(), cart.stem));
            }
            Outcome::Failed(why) => {
                summary.failed += 1;
                report.push_str(&format!("failed    {}/{}: {why}\n", cart.platform.dir_name(), cart.stem));
            }
        }
    }
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
    Saved(String, ThumbKind, PathBuf),
    NoMatch,
    Failed(String),
}

/// Platforms whose listings may hold a cart's art: GB and GBC games are often
/// filed under the other one (Tetris DX is a GBC game many keep with GB games).
fn platforms_for(platform: Platform) -> &'static [Platform] {
    match platform {
        Platform::Gba => &[Platform::Gba],
        Platform::Gb => &[Platform::Gb, Platform::Gbc],
        Platform::Gbc => &[Platform::Gbc, Platform::Gb],
    }
}

fn fetch_one(
    root: &Path,
    server: &str,
    cart: &Cart,
    kind: ThumbKind,
    lists: &mut Listings,
) -> Result<Outcome, String> {
    for used in kind.with_fallbacks() {
        for &platform in platforms_for(cart.platform) {
            let names = lists.get(platform, used)?;
            let Some(name) = best_match(&cart.stem, names) else {
                continue;
            };
            let name = name.to_string();
            let path = format!(
                "/{}/{}/{}.png",
                url_escape(&thumb_system(platform)),
                used.dir(),
                url_escape(&name)
            );
            let png = match get(server, &path, IMAGE_MAX) {
                Ok(Some(png)) if is_png(&png) => png,
                Ok(Some(_)) => return Ok(Outcome::Failed(format!("{name}: not a PNG"))),
                // Listed but gone: try the next kind.
                Ok(None) => continue,
                Err(e) => return Ok(Outcome::Failed(e)),
            };
            let label = root
                .join("Labels")
                .join(cart.platform.dir_name())
                .join(format!("{}.png", cart.stem));
            if label.exists() {
                return Ok(Outcome::Failed("a label appeared meanwhile; kept it".into()));
            }
            if let Some(dir) = label.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // The shelf crops labels around their centre, which cuts the logo off the top
            // of a box: hand it box art already cut to the label's shape from the top.
            let png = match used {
                ThumbKind::Boxart => {
                    let (w, h) = slot_ui::label_size(cart);
                    top_crop(&png, w, h).unwrap_or(png)
                }
                _ => png,
            };
            return match atomic_write(&label, &png) {
                Ok(()) => Ok(Outcome::Saved(name, used, label)),
                Err(e) => Ok(Outcome::Failed(format!("cannot write label: {e}"))),
            };
        }
    }
    Ok(Outcome::NoMatch)
}

/// Crops a PNG to the aspect of a `w` x `h` label, keeping the top of a tall
/// picture (where a box's logo is) or the centre of a wide one.
fn top_crop(png_bytes: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let (rgba, sw, sh) = decode_rgba(png_bytes)?;
    let (cw, ch) = if sw as u64 * h as u64 > sh as u64 * w as u64 {
        ((sh as u64 * w as u64 / h as u64) as u32, sh)
    } else {
        (sw, (sw as u64 * h as u64 / w as u64) as u32)
    };
    if cw == 0 || ch == 0 || (cw, ch) == (sw, sh) {
        return None;
    }
    let x0 = (sw - cw) / 2;
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in 0..ch {
        let at = ((y * sw + x0) * 4) as usize;
        out.extend_from_slice(&rgba[at..at + (cw * 4) as usize]);
    }
    let mut bytes = Vec::new();
    let mut e = png::Encoder::new(&mut bytes, cw, ch);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().ok()?.write_image_data(&out).ok()?;
    Some(bytes)
}

fn decode_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::normalize_to_color8());
    dec.set_limits(png::Limits { bytes: 64 << 20 });
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let n = (info.width * info.height) as usize;
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == n * 4).then_some((rgba, info.width, info.height))
}

fn is_png(bytes: &[u8]) -> bool {
    let mut dec = png::Decoder::new(bytes);
    dec.set_limits(png::Limits { bytes: 64 << 20 });
    dec.read_info().is_ok_and(|r| {
        let info = r.info();
        info.width > 0 && info.height > 0 && info.width <= 4096 && info.height <= 4096
    })
}

/// Thumbnail directory listings, fetched once per run and cached on the card.
struct Listings<'a> {
    root: &'a Path,
    server: &'a str,
    have: Vec<((Platform, ThumbKind), Vec<String>)>,
}

impl<'a> Listings<'a> {
    fn new(root: &'a Path, server: &'a str) -> Self {
        Listings {
            root,
            server,
            have: Vec::new(),
        }
    }

    fn get(&mut self, platform: Platform, kind: ThumbKind) -> Result<&[String], String> {
        let key = (platform, kind);
        if let Some(at) = self.have.iter().position(|(k, _)| *k == key) {
            return Ok(&self.have[at].1);
        }
        let cache = self
            .root
            .join(CACHE_DIR)
            .join(platform.dir_name())
            .join(format!("{}.txt", kind.dir()));
        let names = match read_fresh(&cache) {
            Some(names) => names,
            None => {
                let path = format!("/{}/{}/", url_escape(&thumb_system(platform)), kind.dir());
                let html = get(self.server, &path, LISTING_MAX)
                    .map_err(|e| format!("Can't reach thumbnail server: {e}"))?
                    .unwrap_or_default();
                let names = parse_listing(&String::from_utf8_lossy(&html));
                if let Some(dir) = cache.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = atomic_write(&cache, names.join("\n").as_bytes());
                names
            }
        };
        self.have.push((key, names));
        Ok(&self.have.last().expect("just pushed").1)
    }
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

/// GET over HTTP/1.0 (so no chunked bodies). Ok(None) is a 404.
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

fn request(server: &str, path: &str, limit: usize) -> Result<Reply, String> {
    let host = server.rsplit_once(':').map_or(server, |(h, _)| h);
    let addrs: Vec<_> = server
        .to_socket_addrs()
        .map_err(|_| format!("cannot look up {host} (no internet?)"))?
        .collect();
    let mut stream = None;
    for addr in &addrs {
        if let Ok(s) = TcpStream::connect_timeout(addr, Duration::from_secs(8)) {
            stream = Some(s);
            break;
        }
    }
    let mut stream = stream.ok_or_else(|| format!("cannot connect to {host}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(8)));
    // One write: a request split over several packets can be answered and reset early.
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: slot-brick\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("send failed: {e}"))?;
    let mut raw = Vec::new();
    stream
        .take(limit as u64 + 64 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| format!("download failed: {e}"))?;
    parse_reply(&raw, host, limit)
}

fn parse_reply(raw: &[u8], host: &str, limit: usize) -> Result<Reply, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("bad reply from server")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("bad reply from server")?;
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    match status {
        200 => {
            if body.len() > limit {
                return Err("file too large".into());
            }
            if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
                if body.len() < len {
                    return Err("download cut short".into());
                }
            }
            Ok(Reply::Body(body.to_vec()))
        }
        404 => Ok(Reply::NotFound),
        301 | 302 | 303 | 307 | 308 => {
            let to = header("location").ok_or("redirect without a location")?;
            let prefix = format!("http://{host}");
            match to.strip_prefix(&prefix) {
                Some(rest) if rest.starts_with('/') => Ok(Reply::Moved(rest.to_string())),
                _ if to.starts_with('/') => Ok(Reply::Moved(to)),
                _ => Err(format!("server moved to {to}, which is not supported")),
            }
        }
        _ => Err(format!("server said {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        addr
    }

    fn png_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 2, 2);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[200; 16]).unwrap();
        out
    }

    fn listing(names: &[&str]) -> Vec<u8> {
        names
            .iter()
            .map(|n| format!("<a href=\"{}.png\">x</a>\n", url_escape(n)))
            .collect::<String>()
            .into_bytes()
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
    fn missing_labels_are_fetched_matched_and_reported() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        rom(root, "GBA/pokemon_emerald.gba");
        rom(root, "GBA/Unknown Homebrew.gba");
        rom(root, "GB/Tetris DX.gbc");
        rom(root, "GBA/Golden Sun.gba");
        std::fs::create_dir_all(root.join("Labels/GBA")).unwrap();
        std::fs::write(root.join("Labels/GBA/Golden Sun.png"), b"mine").unwrap();
        std::fs::create_dir_all(root.join("Logs")).unwrap();
        let gba = "/Nintendo%20-%20Game%20Boy%20Advance";
        let gbc = "/Nintendo%20-%20Game%20Boy%20Color";
        let server = serve(vec![
            (format!("{gba}/Named_Boxarts/"), 200, listing(&["Pokemon - Emerald Version (USA, Europe)", "Golden Sun (USA, Europe)"])),
            (format!("{gba}/Named_Boxarts/Pokemon%20-%20Emerald%20Version%20%28USA%2C%20Europe%29.png"), 200, png_bytes()),
            (format!("{gbc}/Named_Boxarts/"), 200, listing(&["Tetris DX (World) (SGB Enhanced) (GB Compatible)"])),
            (format!("{gbc}/Named_Boxarts/Tetris%20DX%20%28World%29%20%28SGB%20Enhanced%29%20%28GB%20Compatible%29.png"), 200, png_bytes()),
        ]);
        let job = ArtJob::start_at(root.to_path_buf(), ThumbKind::Boxart, server);
        let (got, done) = drain(&job);
        assert_eq!(done, Ok(Summary { found: 2, missing: 1, failed: 0 }));
        assert!(got.iter().any(|f| f.stem == "pokemon_emerald" && f.platform == Platform::Gba));
        assert!(got.iter().any(|f| f.stem == "Tetris DX" && f.platform == Platform::Gb));
        assert!(is_png(&std::fs::read(root.join("Labels/GBA/pokemon_emerald.png")).unwrap()));
        assert!(root.join("Labels/GB/Tetris DX.png").exists(), "saved on the cart's own shelf");
        assert_eq!(std::fs::read(root.join("Labels/GBA/Golden Sun.png")).unwrap(), b"mine");
        let report = std::fs::read_to_string(root.join(REPORT)).unwrap();
        assert!(report.contains("no match  GBA/Unknown Homebrew"), "{report}");
        assert!(root.join(CACHE_DIR).join("GBA/Named_Boxarts.txt").exists());
    }

    #[test]
    fn an_unreachable_server_stops_with_a_message() {
        let d = tempfile::tempdir().unwrap();
        rom(d.path(), "GBA/Game.gba");
        let lists = &mut Listings::new(d.path(), "127.0.0.1:1");
        let err = lists.get(Platform::Gba, ThumbKind::Boxart).unwrap_err();
        assert!(err.starts_with("Can't reach thumbnail server"), "{err}");
    }

    #[test]
    fn redirects_on_the_same_host_are_followed_and_others_refused() {
        let moved = b"HTTP/1.1 301 Moved\r\nLocation: http://h/new/\r\n\r\n";
        assert!(matches!(parse_reply(moved, "h", 10), Ok(Reply::Moved(p)) if p == "/new/"));
        let tls = b"HTTP/1.1 301 Moved\r\nLocation: https://h/new/\r\n\r\n";
        assert!(parse_reply(tls, "h", 10).is_err());
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc";
        assert!(parse_reply(short, "h", 100).is_err());
    }

    /// Against the real server: `SLOT_LIVE_ART=1 cargo test ... live_server -- --ignored`.
    #[test]
    #[ignore = "needs internet"]
    fn live_server() {
        if std::env::var_os("SLOT_LIVE_ART").is_none() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        for rom_name in [
            "GBA/Pokemon - Emerald Version (USA, Europe).gba",
            "GBA/metroid_zero_mission.gba",
            "GBA/Legend of Zelda, The - The Minish Cap (U) [!].gba",
            "GBA/Mario & Luigi - Superstar Saga (USA).gba",
            "GB/Tetris DX.gbc",
            "GB/Pokemon Red.gb",
            "GBC/Pokemon - Crystal Version.gbc",
            "GBC/Not A Real Game.gbc",
        ] {
            rom(root, rom_name);
        }
        std::fs::create_dir_all(root.join("Logs")).unwrap();
        let (got, done) = drain(&ArtJob::start(root.to_path_buf(), ThumbKind::Boxart));
        eprintln!("{}", std::fs::read_to_string(root.join(REPORT)).unwrap_or_default());
        assert_eq!(done, Ok(Summary { found: 7, missing: 1, failed: 0 }));
        assert_eq!(got.len(), 7);
        // SLOT_ART_PREVIEW=<dir>: render each cart as the shelf would, to judge the crop.
        if let Some(out) = std::env::var_os("SLOT_ART_PREVIEW") {
            for mut cart in scan(root).unwrap().into_iter().filter(|c| c.label.is_some()) {
                cart.read_header();
                let face = slot_ui::cart_face_with(&cart, crate::label_cache::label_art(root, &cart));
                let path = Path::new(&out).join(format!("{}-{}.png", cart.platform.dir_name(), cart.stem));
                let mut e = png::Encoder::new(std::fs::File::create(path).unwrap(), face.w, face.h);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&face.rgba).unwrap();
            }
        }
    }

    #[test]
    fn box_art_is_cut_to_the_label_from_the_top() {
        // 4x8, top half red, bottom half blue: a 4x2 label keeps only red.
        let mut px = vec![0u8; 4 * 8 * 4];
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(if i < 16 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
        let mut src = Vec::new();
        let mut e = png::Encoder::new(&mut src, 4, 8);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&px).unwrap();
        let (rgba, w, h) = decode_rgba(&top_crop(&src, 4, 2).unwrap()).unwrap();
        assert_eq!((w, h), (4, 2));
        assert!(rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        assert!(top_crop(&src, 1, 2).is_none(), "already the right shape");
    }

    #[test]
    fn a_web_page_is_not_saved_as_a_label() {
        assert!(!is_png(b"<html>not found</html>"));
        assert!(is_png(&png_bytes()));
    }
}
