//! Serialized, generation-owned networking: stale cancellation cannot shut down
//! a newer attempt. Only Host/Join powers up the radio; menu warmup is a no-op.
use std::{path::Path, process::Command, sync::{Mutex, atomic::{AtomicU64, Ordering}}, time::{Duration, Instant}, thread};
use std::os::unix::process::CommandExt;
unsafe extern "C" { fn kill(pid:i32,sig:i32)->i32; }
fn stop_child(child: &mut std::process::Child) { unsafe { kill(-(child.id() as i32),9); } let _=child.wait(); }
use crate::{brick_wifi::{Control, Supplicant}, link_net::Cancel, link_radio::{LinkRole, RadioFail}};
static GENERATION: AtomicU64 = AtomicU64::new(0);
static NETWORK: Mutex<()> = Mutex::new(());
pub fn current() -> u64 { GENERATION.load(Ordering::SeqCst) }
pub fn begin() -> u64 { GENERATION.fetch_add(1, Ordering::SeqCst)+1 }
fn script(verb: &str, cancel: Option<&Cancel>) -> Result<(), String> {
    let root=std::env::var_os("SLOT_ROOT").unwrap_or_else(||"/mnt/SDCARD/Slot".into());
    let mut child=Command::new("/bin/sh").arg(Path::new(&root).join("System/slotlink.sh")).args(["link",verb]).process_group(0).spawn().map_err(|e|e.to_string())?;
    let deadline=Instant::now()+Duration::from_secs(20);
    loop {
        if cancel.is_some_and(Cancel::is_cancelled) || Instant::now()>=deadline {
            stop_child(&mut child); return Err("Wi-Fi setup cancelled or timed out".into());
        }
        match child.try_wait() {
            Ok(Some(s)) => return if s.success() {Ok(())} else {Err(format!("Wi-Fi helper: {s}; see Logs/multiplayer.log"))},
            Ok(None)=>thread::sleep(Duration::from_millis(50)),
            Err(e)=>{stop_child(&mut child);return Err(e.to_string());}
        }
    }
}
pub fn down(token: u64) {
    let _lock=NETWORK.lock().unwrap_or_else(|e|e.into_inner());
    if token==current() { if let Err(e)=script("down",None) { eprintln!("slot: multiplayer cleanup: {e}"); } }
}
pub fn quiesce() { let token=begin(); down(token); }
fn ready(role: LinkRole, status: &str) -> bool {
    status.lines().any(|s|s==match role {LinkRole::Host=>"state=ENABLED",LinkRole::Join=>"wpa_state=COMPLETED"})
}
pub fn up(token: u64, role: LinkRole, cancel: &Cancel) -> Result<(),RadioFail> {
    let _lock=NETWORK.lock().unwrap_or_else(|e|e.into_inner());
    if token!=current() || cancel.is_cancelled() {return Err(RadioFail::Cancelled)}
    let result=(||{
        script(role.arg(),Some(cancel)).map_err(RadioFail::Radio)?;
        let root=std::env::var("SLOT_LINK_RUN").unwrap_or_else(|_|"/tmp/slot-brick-link".into());
        let socket=Path::new(&root).join(match role {LinkRole::Host=>"host/wlan0",LinkRole::Join=>"join/wlan0"});
        let deadline=Instant::now()+Duration::from_secs(20);
        while Instant::now()<deadline {
            if cancel.is_cancelled() || token!=current(){return Err(RadioFail::Cancelled)}
            if let Ok(c)=Control::at(&socket) { if c.command("STATUS").is_ok_and(|s|ready(role,&s)) {return Ok(())} }
            thread::sleep(Duration::from_millis(200));
        }
        Err(if role==LinkRole::Join {RadioFail::NoHost}else{RadioFail::Radio("Access point did not become ready; see Logs/multiplayer.log".into())})
    })();
    if result.is_err() {let _=script("down",None);}
    result
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn require_real_driver_readiness() {
        assert!(ready(LinkRole::Host,"state=ENABLED\n"));
        assert!(!ready(LinkRole::Host,"state=DISABLED\n"));
        assert!(ready(LinkRole::Join,"wpa_state=COMPLETED\n"));
        assert!(!ready(LinkRole::Join,"wpa_state=SCANNING\n"));
    }
    #[test] fn generations_invalidate_old_owners() { let a=begin();let b=begin();assert!(b>a);assert_ne!(a,current());assert_eq!(b,current()); }
}
