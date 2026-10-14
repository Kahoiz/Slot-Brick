use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slot_cheevos::{Cheevos, Console, Http, HttpRequest, Notice};

/// Answers like retroachievements.org would, by the request's `r=`.
#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<String>>,
}

impl Http for Fake {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        let post = request.post.clone().unwrap_or_default();
        self.seen.lock().unwrap().push(post.clone());
        let r = post.split('&').find_map(|kv| kv.strip_prefix("r=")).unwrap_or("");
        let body = match r {
            "login2" => r#"{"Success":true,"User":"Benni","Token":"t0k3n","Score":10,"SoftcoreScore":2,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#,
            _ => r#"{"Success":false,"Error":"Unknown game","Code":"not_found","Status":404}"#,
        };
        Ok((if r == "login2" { 200 } else { 404 }, body.as_bytes().to_vec()))
    }
}

fn wait(c: &Cheevos, mut want: impl FnMut(&Notice) -> bool) -> Notice {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(10) {
        for n in c.take() {
            if want(&n) {
                return n;
            }
            seen.push(n);
        }
        c.idle();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no such notice; saw {seen:?}");
}

#[test]
fn rcheevos_is_the_vendored_release() {
    assert_eq!(slot_cheevos::version(), "12.5");
}

#[test]
fn a_token_logs_in_and_a_rom_is_identified_by_its_hash() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    assert_eq!(wait(&c, |n| matches!(n, Notice::LoggedIn(_))), Notice::LoggedIn(Ok(())));
    assert!(fake.seen.lock().unwrap()[0].contains("u=Benni") && fake.seen.lock().unwrap()[0].contains("t=t0k3n"));

    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.gba");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    c.load_game(&rom, Console::Gba);
    assert!(matches!(wait(&c, |n| matches!(n, Notice::GameLoaded(_))), Notice::GameLoaded(Err(_))));
    let seen = fake.seen.lock().unwrap();
    assert!(seen.iter().any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")), "{seen:?}");
}

#[test]
fn a_refused_token_says_so() {
    struct No;
    impl Http for No {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            Ok((401, br#"{"Success":false,"Error":"Invalid user/token combination.","Code":"invalid_credentials","Status":401}"#.to_vec()))
        }
    }
    let c = Cheevos::new(Arc::new(No)).unwrap();
    c.login("Benni", "old");
    match wait(&c, |n| matches!(n, Notice::LoggedIn(_))) {
        Notice::LoggedIn(Err(e)) => assert!(e.contains("Invalid"), "{e}"),
        n => panic!("{n:?}"),
    }
}

#[test]
fn core_memory_is_mapped_for_a_game_boy() {
    let c = Cheevos::new(Arc::new(Fake::default()));
    let mut c = c.unwrap();
    let mut wram = vec![0u8; 0x2000];
    let mut hram = vec![0u8; 0x80];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::Gb, &mut |id| match id {
            2 => Some((wram.as_mut_ptr(), wram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x2000, "{mapped}");
    let _ = &mut hram;
}

#[test]
fn dropping_with_a_request_in_flight_does_not_crash() {
    struct Slow;
    impl Http for Slow {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            std::thread::sleep(Duration::from_millis(300));
            Err("offline".into())
        }
    }
    let c = Cheevos::new(Arc::new(Slow)).unwrap();
    c.login("Benni", "t");
    drop(c);
    std::thread::sleep(Duration::from_millis(600));
}
