//! RetroAchievements account: log in once with the password, keep only the token the
//! server hands back, and check it still works. Always over HTTPS, since both the
//! password and the token would otherwise cross the network in the clear.
//!
//! Protocol: dorequest.php, `r=login2` (as rcheevos and RetroArch use it).
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::net;

const SERVER: &str = "retroachievements.org:443";
const PATH: &str = "/dorequest.php";
/// Who is logged in. Holds a login token, never the password.
pub const ACCOUNT_FILE: &str = "Config/retroachievements.txt";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub token: String,
}

/// What the server says about an account, and how long it took to say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub user: String,
    pub score: i64,
    pub softcore: i64,
    pub ms: u128,
}

fn file(root: &Path) -> PathBuf {
    root.join(ACCOUNT_FILE)
}

pub fn load(root: &Path) -> Option<Account> {
    let text = std::fs::read_to_string(file(root)).ok()?;
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    Some(Account {
        user: field("user")?,
        token: field("token")?,
    })
}

pub fn save(root: &Path, account: &Account) -> std::io::Result<()> {
    let path = file(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A newline in either would write a second field into the file.
    if [&account.user, &account.token].iter().any(|v| v.contains(['\n', '\r'])) {
        return Err(std::io::Error::other("bad user name or token"));
    }
    let text = format!(
        "# RetroAchievements login token for Slot. Delete this file to log out.\nuser={}\ntoken={}\n",
        account.user, account.token
    );
    slot_store::atomic_write(&path, text.as_bytes())?;
    // Owner-only where the filesystem keeps modes (the card's FAT does not).
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok(())
}

pub fn forget(root: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(file(root)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// rcheevos' requests, over slot's own HTTPS.
pub struct Http;

impl slot_cheevos::Http for Http {
    fn send(&self, request: &slot_cheevos::HttpRequest) -> Result<(u16, Vec<u8>), String> {
        // Every request carries the login token: never in the clear.
        let (tls, server, path) = net::split_url(&request.url).ok_or_else(|| format!("unsupported URL {}", request.url))?;
        if !tls {
            return Err(format!("refused, not HTTPS: {}", request.url));
        }
        let method = if request.post.is_some() { "POST" } else { "GET" };
        let reply = net::send(&net::Request {
            tls,
            server: &server,
            method,
            path: &path,
            form: request.post.as_deref(),
            limit: 4 << 20,
        })?;
        Ok((reply.status, reply.body))
    }
}

/// Logs in with a password; the password is sent once and never kept.
pub fn login(user: &str, password: &str) -> Result<(Account, Profile), String> {
    let (profile, token) = login2(&format!(
        "r=login2&u={}&p={}",
        net::form_escape(user),
        net::form_escape(password)
    ))?;
    let token = token.ok_or("the server sent no login token")?;
    Ok((
        Account {
            user: profile.user.clone(),
            token,
        },
        profile,
    ))
}

/// Checks a saved login against the server: the connectivity test.
pub fn check(account: &Account) -> Result<Profile, String> {
    login2(&format!(
        "r=login2&u={}&t={}",
        net::form_escape(&account.user),
        net::form_escape(&account.token)
    ))
    .map(|(profile, _)| profile)
}

fn login2(form: &str) -> Result<(Profile, Option<String>), String> {
    let started = Instant::now();
    let reply = net::send(&net::Request {
        tls: true,
        server: SERVER,
        method: "POST",
        path: PATH,
        form: Some(form),
        limit: 1 << 16,
    })?;
    let ms = started.elapsed().as_millis();
    let body = String::from_utf8_lossy(&reply.body);
    parse_login(&body, reply.status, ms)
}

fn parse_login(body: &str, status: u16, ms: u128) -> Result<(Profile, Option<String>), String> {
    if json_bool(body, "Success") != Some(true) {
        return Err(json_str(body, "Error").unwrap_or_else(|| format!("RetroAchievements said {status}")));
    }
    let profile = Profile {
        user: json_str(body, "User").ok_or("the server sent no user name")?,
        score: json_num(body, "Score").unwrap_or(0),
        softcore: json_num(body, "SoftcoreScore").unwrap_or(0),
        ms,
    };
    Ok((profile, json_str(body, "Token")))
}

/// The raw text after `"key":` in a flat JSON object.
fn json_raw<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let at = body.find(&format!("\"{key}\""))?;
    let rest = body[at + key.len() + 2..].trim_start().strip_prefix(':')?;
    Some(rest.trim_start())
}

fn json_str(body: &str, key: &str) -> Option<String> {
    let raw = json_raw(body, key)?.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('?'));
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    None
}

fn json_num(body: &str, key: &str) -> Option<i64> {
    let raw = json_raw(body, key)?;
    let end = raw.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(raw.len());
    raw[..end].parse().ok()
}

fn json_bool(body: &str, key: &str) -> Option<bool> {
    let raw = json_raw(body, key)?;
    if raw.starts_with("true") {
        Some(true)
    } else if raw.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_reply_gives_the_profile_and_token() {
        let body = r#"{"Success":true,"User":"Benni","AvatarUrl":"x","Token":"abc\/123","Score":1234,"SoftcoreScore":56,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#;
        let (p, token) = parse_login(body, 200, 80).unwrap();
        assert_eq!(p, Profile { user: "Benni".into(), score: 1234, softcore: 56, ms: 80 });
        assert_eq!(token.as_deref(), Some("abc/123"));
    }

    #[test]
    fn a_refusal_says_why() {
        let body = r#"{"Success":false,"Status":401,"Code":"invalid_credentials","Error":"Invalid user\/password combination. Please try again."}"#;
        assert_eq!(parse_login(body, 401, 1).unwrap_err(), "Invalid user/password combination. Please try again.");
        assert_eq!(parse_login("<html>", 502, 1).unwrap_err(), "RetroAchievements said 502");
    }

    #[test]
    fn the_account_round_trips_and_logs_out() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(d.path()), None);
        let a = Account { user: "Benni".into(), token: "t0k3n".into() };
        save(d.path(), &a).unwrap();
        assert_eq!(load(d.path()), Some(a));
        assert!(!std::fs::read_to_string(file(d.path())).unwrap().contains("password"));
        forget(d.path()).unwrap();
        assert_eq!(load(d.path()), None);
        forget(d.path()).unwrap();
    }

    /// `SLOT_LIVE_NET=1 ... live_bad_login -- --ignored`: the real server refuses a bad login.
    #[test]
    #[ignore = "needs internet"]
    fn live_bad_login() {
        if std::env::var_os("SLOT_LIVE_NET").is_none() {
            return;
        }
        let err = login("slot-brick-test-no-such-user", "wrong").unwrap_err();
        assert!(!err.contains("connect") && !err.contains("secure"), "{err}");
    }
}
