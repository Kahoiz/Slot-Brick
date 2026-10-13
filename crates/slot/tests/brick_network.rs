#![cfg(feature="brick")]
use std::{fs,thread,time::Duration};
use slot::{brick_link,link_radio::{LinkRole,RadioFail},link_net::Cancel};
#[test]
fn network_lifecycle_and_stale_cleanup_are_serialized() {
    let tmp=tempfile::tempdir().unwrap(); let root=tmp.path();fs::create_dir(root.join("System")).unwrap();
    let run=root.join("net");fs::create_dir_all(run.join("host")).unwrap();fs::create_dir_all(run.join("join")).unwrap();
    unsafe {std::env::set_var("SLOT_ROOT",root);std::env::set_var("SLOT_LINK_RUN",&run);}
    let helper=root.join("System/slotlink.sh");
    fs::write(&helper,"echo $2 >> \"$SLOT_ROOT/calls\"\n").unwrap();
    let token=brick_link::begin();let newer=brick_link::begin();
    brick_link::down(token);assert!(!root.join("calls").exists());
    brick_link::down(newer);assert_eq!(fs::read_to_string(root.join("calls")).unwrap(),"down\n");
    // Cancelling setup kills its process group: a delayed grandchild must not restart Wi-Fi.
    fs::write(&helper,"echo $2 >> \"$SLOT_ROOT/calls\"\nif [ \"$2\" = host ]; then sleep 0.5; echo LATE >> \"$SLOT_ROOT/calls\"; fi\n").unwrap();
    let c=Cancel::new();let flag=c.clone();let token=brick_link::begin();
    let t=thread::spawn(move||brick_link::up(token,LinkRole::Host,&flag));thread::sleep(Duration::from_millis(100));c.cancel();assert!(t.join().unwrap().is_err());thread::sleep(Duration::from_millis(550));
    let calls=fs::read_to_string(root.join("calls")).unwrap();assert!(!calls.contains("LATE"));assert!(calls.ends_with("down\n"));
    let old=brick_link::begin();brick_link::begin();assert_eq!(brick_link::up(old,LinkRole::Join,&Cancel::new()),Err(RadioFail::Cancelled));
    fs::write(&helper,"echo $2 >> \"$SLOT_ROOT/calls\"\n[ \"$2\" = down ]\n").unwrap();
    assert!(brick_link::up(brick_link::begin(),LinkRole::Host,&Cancel::new()).is_err());assert!(fs::read_to_string(root.join("calls")).unwrap().ends_with("down\n"));
    unsafe {std::env::remove_var("SLOT_ROOT");std::env::remove_var("SLOT_LINK_RUN");}
}
