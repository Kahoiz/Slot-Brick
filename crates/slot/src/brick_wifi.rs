//! Brick Wi-Fi settings. Uses the firmware supplicant; no shell interpolation of credentials.
use std::{fs,os::unix::{net::UnixDatagram,fs::PermissionsExt},path::{Path,PathBuf},process::{Command,Stdio},sync::{mpsc,Arc,atomic::{AtomicBool,AtomicU64,Ordering}},thread,time::{Duration,Instant}};
use slot_input::{Action,Btn};
use slot_ui::{TexId,UndoFace};
type Result<T> = std::result::Result<T,String>;
static SERIAL:AtomicU64=AtomicU64::new(0);
#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum Security {Open,Personal,Unsupported}
#[derive(Clone,Debug)]
struct Network {ssid:Vec<u8>,signal:i32,security:Security,saved:Option<u32>}
fn decode(s:&str)->Vec<u8> {
    let b=s.as_bytes();let mut v=Vec::new();let mut i=0;
    while i<b.len(){
        if b[i]==b'\\' && i+1<b.len(){
            if b[i+1]==b'x' && i+3<b.len(){if let Some(n)=std::str::from_utf8(&b[i+2..i+4]).ok().and_then(|s|u8::from_str_radix(s,16).ok()){v.push(n);i+=4;continue}}
            let c=match b[i+1]{b'\\'=>Some(b'\\'),b'"'=>Some(b'"'),b'n'=>Some(b'\n'),b't'=>Some(b'\t'),b'r'=>Some(b'\r'),_=>None};
            if let Some(c)=c {v.push(c);i+=2;continue}
        }v.push(b[i]);i+=1;
    }v
}
fn name(b:&[u8])->String{String::from_utf8_lossy(b).chars().map(|c|if c.is_control(){' '}else{c}).collect()}
fn hex(b:&[u8])->String{b.iter().map(|b|format!("{b:02x}")).collect()}
fn psk(s:&str)->Result<String>{
    if s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit()){return Ok(s.into())}
    if !(8..=63).contains(&s.len()) || !s.bytes().all(|b|(32..=126).contains(&b)){return Err("Use 8-63 characters or 64 hex digits".into())}
    // wpa_config_parse_psk takes the literal bytes between the first and last quote.
    // Backslashes and interior quotes must NOT be C-escaped here.
    Ok(format!("\"{s}\""))
}
fn scan_rows(scan:&str,saved:&str)->Vec<Network>{
    let saved:Vec<_>=saved.lines().skip(1).filter_map(|s|{let v:Vec<_>=s.split('\t').collect();Some((v.first()?.parse::<u32>().ok()?,decode(v.get(1)?)))}).collect();
    let mut out:Vec<Network>=Vec::new();
    for s in scan.lines().skip(1){let v:Vec<_>=s.splitn(5,'\t').collect();if v.len()!=5{continue}let ssid=decode(v[4]);if ssid.is_empty() || ssid.len()>32{continue}
        let security=if v[3].contains("PSK"){Security::Personal}else if ["WPA","WEP","EAP","SAE","OWE","RSN"].iter().any(|s|v[3].contains(s)){Security::Unsupported}else{Security::Open};
        let n=Network{saved:saved.iter().find(|(_,s)|s==&ssid).map(|(id,_)|*id),ssid,security,signal:v[2].parse().unwrap_or(-100)};
        if let Some(old)=out.iter_mut().find(|a|a.ssid==n.ssid && a.security==n.security){if old.signal<n.signal{*old=n}}else{out.push(n)}
    }out.sort_by(|a,b|b.signal.cmp(&a.signal).then(a.ssid.cmp(&b.ssid)));out
}
pub(crate) trait Supplicant {
    fn command(&self,s:&str)->Result<String>;
    fn ok(&self,s:&str)->Result<()>{if self.command(s)?=="OK"{Ok(())}else{Err("Wi-Fi service rejected the request".into())}}
}
pub(crate) struct Control{socket:UnixDatagram,local:PathBuf}
impl Drop for Control{fn drop(&mut self){let _=fs::remove_file(&self.local);}}
impl Control{
    pub(crate) fn at(remote:&Path)->Result<Self>{
        let local=PathBuf::from(format!("/tmp/slot-wifi-{}-{}",std::process::id(),SERIAL.fetch_add(1,Ordering::Relaxed)));
        let socket=UnixDatagram::bind(&local).map_err(|_|"Cannot open Wi-Fi control socket")?;
        let c=Self{socket,local};fs::set_permissions(&c.local,fs::Permissions::from_mode(0o600)).map_err(|_|"Cannot secure Wi-Fi socket")?;
        c.socket.set_read_timeout(Some(Duration::from_secs(2))).map_err(|_|"Cannot set Wi-Fi timeout")?;
        c.socket.set_write_timeout(Some(Duration::from_secs(2))).map_err(|_|"Cannot set Wi-Fi timeout")?;
        c.socket.connect(remote).map_err(|_|"Wi-Fi service unavailable")?;Ok(c)
    }
    fn open()->Result<Self>{["/etc/wifi/sockets/wlan0","/var/run/wpa_supplicant/wlan0","/run/wpa_supplicant/wlan0"].iter().find_map(|s|Self::at(Path::new(s)).ok()).ok_or("Wi-Fi service unavailable".into())}
}
impl Supplicant for Control{
    fn command(&self,s:&str)->Result<String>{
        self.socket.send(s.as_bytes()).map_err(|_|"Wi-Fi command failed")?;let mut b=[0;32768];let n=self.socket.recv(&mut b).map_err(|_|"Wi-Fi service did not respond")?;
        let s=String::from_utf8_lossy(&b[..n]).trim_end_matches('\n').to_string();
        if s.starts_with("FAIL") || s.starts_with("UNKNOWN"){Err("Wi-Fi service rejected the request".into())}else{Ok(s)}
    }
}
fn field<'a>(s:&'a str,key:&str)->Option<&'a str>{s.lines().find_map(|s|s.strip_prefix(key).and_then(|s|s.strip_prefix('=')))}
fn pause(cancel:&AtomicBool,ms:u64)->Result<()>{for _ in 0..(ms+99)/100{if cancel.load(Ordering::Relaxed){return Err("Cancelled".into())}thread::sleep(Duration::from_millis(100));}Ok(())}
/// Stock Brick firmware ships no /etc/wifi/wifi_init.sh, so slot carries its own
/// System/brick_wifi.sh. A firmware helper, where one exists, still wins.
fn helper(firmware:&Path,root:&Path)->Option<PathBuf>{
    if firmware.is_file(){return Some(firmware.into())}
    let own=root.join("System/brick_wifi.sh");own.is_file().then_some(own)
}
fn service(on:bool)->Result<()>{
    let root=PathBuf::from(std::env::var_os("SLOT_ROOT").unwrap_or_else(||"/mnt/SDCARD/Slot".into()));
    let Some(script)=helper(Path::new("/etc/wifi/wifi_init.sh"),&root) else {return Err("Wi-Fi helper missing: System/brick_wifi.sh".into())};
    let mut child=Command::new("/bin/sh").arg(&script).arg(if on{"start"}else{"stop"}).env("SLOT_ROOT",&root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_|"Cannot start Wi-Fi helper")?;
    let start=Instant::now();loop{
        if let Some(status)=child.try_wait().map_err(|_|"Cannot check Wi-Fi helper")?{return if status.success(){Ok(())}else{Err("Could not switch Wi-Fi; see Logs/wifi.log".into())}}
        if start.elapsed()>Duration::from_secs(15){let _=child.kill();let _=child.wait();return Err("Wi-Fi helper timed out; see Logs/wifi.log".into())}
        thread::sleep(Duration::from_millis(100));
    }
}
fn connect(c:&impl Supplicant,n:&Network,password:Option<String>,cancel:&AtomicBool)->Result<String>{
    let old=c.command("LIST_NETWORKS")?;
    let previous:Vec<(u32,bool,bool)>=old.lines().skip(1).filter_map(|s|{let v:Vec<_>=s.split('\t').collect();Some((v.first()?.parse().ok()?,!v.last()?.contains("[DISABLED]"),v.last()?.contains("[CURRENT]")))}).collect();
    let fresh=password.is_some() || n.saved.is_none();
    let secret=if fresh && n.security==Security::Personal{Some(psk(password.as_deref().unwrap_or(""))?)}else{None};
    let id=if fresh{c.command("ADD_NETWORK")?.parse::<u32>().map_err(|_|"Cannot create Wi-Fi profile")?}else{n.saved.unwrap()};
    let result=(||{
        if fresh{c.ok(&format!("SET_NETWORK {id} ssid {}",hex(&n.ssid)))?;c.ok(&format!("SET_NETWORK {id} key_mgmt {}",if n.security==Security::Open{"NONE"}else{"WPA-PSK"}))?;if let Some(secret)=secret{c.ok(&format!("SET_NETWORK {id} psk {secret}"))?;}}
        c.ok(&format!("SELECT_NETWORK {id}"))?;
        let deadline=Instant::now()+Duration::from_secs(20);
        while Instant::now()<deadline{
            pause(cancel,500)?;let s=c.command("STATUS")?;
            if field(&s,"id").and_then(|s|s.parse::<u32>().ok())==Some(id) && field(&s,"wpa_state")==Some("COMPLETED"){
                // Restore eligibility of previously enabled saved networks.
                for &(old,on,_) in &previous{if on{c.ok(&format!("ENABLE_NETWORK {old}"))?;}}
                // Keep the previous profile until a successful replacement has been saved.
                if c.ok("SAVE_CONFIG").is_err(){return Ok("Connected; firmware could not save profile".into())}
                if fresh{if let Some(old)=n.saved{if old!=id && c.ok(&format!("REMOVE_NETWORK {old}")).is_ok(){let _=c.ok("SAVE_CONFIG");}}}
                return Ok(if field(&s,"ip_address").is_some(){"Connected; network saved"}else{"Connected; waiting for IP address"}.into())
            }
        }Err("Connection failed. Check password / signal".into())
    })();
    if result.is_err(){
        if fresh{let _=c.ok(&format!("REMOVE_NETWORK {id}"));}
        if let Some(&(id,_,_))=previous.iter().find(|(_,_,current)|*current){let _=c.ok(&format!("SELECT_NETWORK {id}"));}else{let _=c.ok("DISCONNECT");}
        for &(id,on,_) in &previous{let _=c.ok(&format!("{}_NETWORK {id}",if on{"ENABLE"}else{"DISABLE"}));}
    }result
}
enum Job{Status,Scan,Power(bool),Connect(Network,Option<String>),Disconnect}
struct Reply{on:bool,status:String,networks:Option<Vec<Network>>}
fn run(job:Job,cancel:&AtomicBool)->Result<Reply>{
    if let Job::Power(on)=job{
        service(on)?;
        if !on{return Ok(Reply{on:false,status:"Wi-Fi off".into(),networks:Some(Vec::new())})}
        for _ in 0..20{if Control::open().is_ok(){break}pause(cancel,250)?;}
        return run(Job::Scan,cancel)
    }
    let c=match Control::open(){Ok(c)=>c,Err(e)=>return if matches!(job,Job::Status){Ok(Reply{on:false,status:"Wi-Fi off / service unavailable".into(),networks:None})}else{Err(e)}};
    let s=c.command("STATUS")?;let connected=field(&s,"wpa_state")==Some("COMPLETED");
    let mut r=Reply{on:true,status:if connected{format!("{}  {}",name(&decode(field(&s,"ssid").unwrap_or("Connected"))),field(&s,"ip_address").unwrap_or("Waiting for IP"))}else{"Not connected".into()},networks:None};
    match job{
        Job::Status=>{},Job::Power(_)=>unreachable!(),
        Job::Scan=>{c.ok("SCAN")?;pause(cancel,3500)?;r.networks=Some(scan_rows(&c.command("SCAN_RESULTS")?,&c.command("LIST_NETWORKS")?));if r.networks.as_ref().unwrap().is_empty(){r.status="No networks found. Try scanning again".into();}},
        Job::Connect(n,p)=>{r.status=connect(&c,&n,p,cancel)?;r.networks=Some(scan_rows(&c.command("SCAN_RESULTS")?,&c.command("LIST_NETWORKS")?));},
        Job::Disconnect=>{c.ok("DISCONNECT")?;r.status="Disconnected".into();},
    }Ok(r)
}
struct Worker{rx:mpsc::Receiver<Result<Reply>>,cancel:Arc<AtomicBool>}
struct Keyboard{network:Network,password:String,key:usize,page:usize}
const KEYS:[&str;3]=["abcdefghijklmnopqrstuvwxyz0123456789    ","ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789    ","!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~        "];
pub struct WifiMenu{worker:Option<Worker>,pub face:Option<TexId>,pub dirty:bool,on:bool,status:String,networks:Vec<Network>,row:usize,keyboard:Option<Keyboard>,refreshed:Instant}
impl WifiMenu{
    pub fn new()->Self{let mut s=Self{worker:None,face:None,dirty:true,on:false,status:"Checking Wi-Fi...".into(),networks:Vec::new(),row:0,keyboard:None,refreshed:Instant::now()};s.start(Job::Status);s}
    pub fn busy(&self)->bool{self.worker.is_some()}
    pub fn cancel(&mut self){if let Some(w)=&self.worker{w.cancel.store(true,Ordering::Relaxed);self.status="Cancelling...".into();self.dirty=true;}}
    fn start(&mut self,job:Job){
        if self.busy(){return}let(tx,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));let token=cancel.clone();
        if !matches!(job,Job::Status){self.status=match job{Job::Scan=>"Scanning...",Job::Power(true)=>"Turning Wi-Fi on...",Job::Power(false)=>"Turning Wi-Fi off...",Job::Connect(..)=>"Connecting...",Job::Disconnect=>"Disconnecting...",_=>unreachable!()}.into();}
        thread::spawn(move||{let _=tx.send(run(job,&token));});self.worker=Some(Worker{rx,cancel});self.dirty=true;
    }
    pub fn poll(&mut self){
        if let Some(w)=&self.worker{match w.rx.try_recv(){
            Ok(r)=>{self.worker=None;match r{Ok(r)=>{self.on=r.on;self.status=r.status;if let Some(n)=r.networks{self.networks=n;}},Err(e)=>self.status=e}self.row=self.row.min(self.networks.len()+2);self.dirty=true;self.refreshed=Instant::now();},
            Err(mpsc::TryRecvError::Disconnected)=>{self.worker=None;self.status="Wi-Fi worker stopped".into();self.dirty=true;self.refreshed=Instant::now();},_=>{}
        }}
        // Poll only while this menu is open; scanning is always explicit.
        if !self.busy() && self.keyboard.is_none() && self.refreshed.elapsed()>Duration::from_secs(15){self.start(Job::Status);}
    }
    pub fn input(&mut self,a:Action)->bool{
        if self.busy(){if matches!(a,Action::GbaDown(Btn::B)){self.cancel();}return false}
        if !matches!(a,Action::GbaDown(_)|Action::ShelfLeft|Action::ShelfRight|Action::Insert){return false}
        self.dirty=true;
        if let Some(k)=&mut self.keyboard{
            match a{
                Action::GbaDown(Btn::B)=>self.keyboard=None,Action::GbaDown(Btn::X)=>{k.password.pop();},Action::GbaDown(Btn::Y)=>k.page=(k.page+1)%3,
                Action::GbaDown(Btn::Up)=>k.key=(k.key+30)%40,Action::GbaDown(Btn::Down)=>k.key=(k.key+10)%40,
                Action::GbaDown(Btn::Left)|Action::ShelfLeft=>k.key=k.key/10*10+(k.key%10+9)%10,
                Action::GbaDown(Btn::Right)|Action::ShelfRight=>k.key=k.key/10*10+(k.key%10+1)%10,
                Action::GbaDown(Btn::A)|Action::Insert=>{if k.password.len()<64{k.password.push(KEYS[k.page].as_bytes()[k.key] as char);}},
                Action::GbaDown(Btn::Start)=>{if let Err(e)=psk(&k.password){self.status=e;}else{let k=self.keyboard.take().unwrap();self.start(Job::Connect(k.network,Some(k.password)));}},_=>{}
            }return false
        }
        match a{
            Action::GbaDown(Btn::B)=>return true,
            Action::GbaDown(Btn::Up)=>self.row=(self.row+self.networks.len()+2)%(self.networks.len()+3),
            Action::GbaDown(Btn::Down)=>self.row=(self.row+1)%(self.networks.len()+3),
            Action::GbaDown(Btn::A)|Action::Insert=>match self.row{0=>self.start(Job::Power(!self.on)),1=>self.start(Job::Scan),2=>self.start(Job::Disconnect),_=>self.choose(false)},
            Action::GbaDown(Btn::X)=>self.choose(true),_=>{}
        }false
    }
    fn choose(&mut self,edit:bool){
        let Some(n)=self.row.checked_sub(3).and_then(|i|self.networks.get(i)).cloned()else{return};
        if n.security==Security::Unsupported{self.status="Use an open or WPA/WPA2 personal network".into();return}
        if n.security==Security::Personal && (edit || n.saved.is_none()){self.status="Password is case sensitive".into();self.keyboard=Some(Keyboard{network:n,password:String::new(),key:0,page:0});}else{self.start(Job::Connect(n,None));}
    }
    pub fn render(&self)->UndoFace{
        let mut p=Painter::new();p.text("WI-FI",32,24,576,30.0);p.text(&self.status,32,66,576,18.0);
        if let Some(k)=&self.keyboard{
            p.text(&name(&k.network.ssid),32,102,576,24.0);p.text(&format!("{}  ({}/64)","*".repeat(k.password.len().min(30)),k.password.len()),32,142,576,21.0);
            for i in 0..40{let x=32+i%10*58;let y=196+i/10*45;if i==k.key{p.rect(x,y,54,41,slot_ui::edge());}let c=&KEYS[k.page][i..i+1];p.text(if c==" "{"_"}else{c},x,y+3,54,26.0);}
            p.text("A TYPE    X DELETE    Y abc / ABC / #!?",24,392,592,18.0);p.text("B BACK                    START CONNECT",24,430,592,20.0);
        }else{
            let first=if self.row<7{0}else{self.row-6};
            for (line,row) in (first..(self.networks.len()+3).min(first+7)).enumerate(){let y=110+line*41;if row==self.row{p.rect(0,y,640,38,slot_ui::edge());}
                let label=match row{0=>format!("WI-FI                          {}",if self.on{"ON"}else{"OFF"}),1=>"SCAN NETWORKS".into(),2=>"DISCONNECT".into(),_=>{let n=&self.networks[row-3];format!("{}  {}  {} dBm",name(&n.ssid),if n.saved.is_some(){"[SAVED]"}else if n.security==Security::Open{"[OPEN]"}else{"[LOCK]"},n.signal)}};p.text(&label,32,y+1,576,23.0);
            }p.text(if self.busy(){"B CANCEL"}else{"B BACK    A SELECT    X EDIT PASSWORD"},24,430,592,20.0);
        }UndoFace{rgba:p.rgba,w:640,h:480}
    }
}
struct Painter{rgba:Vec<u8>}
impl Painter{
    fn new()->Self{let mut s=Self{rgba:vec![0;640*480*4]};s.rect(0,0,640,480,slot_ui::opening());s}
    fn rect(&mut self,x:usize,y:usize,w:usize,h:usize,c:[f32;4]){let c=c.map(|v|(v*255.0)as u8);for yy in y..(y+h).min(480){for xx in x..(x+w).min(640){self.rgba[(yy*640+xx)*4..(yy*640+xx)*4+4].copy_from_slice(&c)}}}
    fn text(&mut self,s:&str,x:usize,y:usize,w:usize,px:f32){let Some(font)=slot_ui::text::label_font()else{return};let mut s=s.to_string();while slot_ui::text::line_width(font,&s,px,0.0)>w as f32 && !s.is_empty(){s.pop();}
        let h=40;let mut face=vec![0;w*h*4];let layout=slot_ui::text::Layout{lines:vec![s],px,tracking:0.0};slot_ui::text::draw_centred(&mut face,w as u32,h as u32,&layout,[238,238,242]);
        for yy in 0..h.min(480-y){for xx in 0..w.min(640-x){let a=(yy*w+xx)*4;let b=((y+yy)*640+x+xx)*4;let alpha=face[a+3]as u32;for ch in 0..3{self.rgba[b+ch]=((face[a+ch]as u32*alpha+self.rgba[b+ch]as u32*(255-alpha))/255)as u8;}}}
    }
}
#[cfg(test)]mod tests{
    use super::*;use std::cell::RefCell;
    struct Mock{steps:RefCell<std::collections::VecDeque<(String,Result<String>)>>}
    impl Mock{fn new(steps:Vec<(&str,Result<&str>)>)->Self{Self{steps:RefCell::new(steps.into_iter().map(|(a,b)|(a.into(),b.map(String::from))).collect())}}fn done(&self){assert!(self.steps.borrow().is_empty());}}
    impl Supplicant for Mock{fn command(&self,s:&str)->Result<String>{let(a,b)=self.steps.borrow_mut().pop_front().expect("unexpected command");assert_eq!(a,s);b}}
    fn network()->Network{Network{ssid:b"Test".to_vec(),signal:-40,security:Security::Personal,saved:None}}
    #[test]fn parse_scans(){let n=scan_rows("header\na\t2412\t-80\t[WPA2-PSK-CCMP]\tMy\\x20WiFi\nb\t2412\t-40\t[WPA2-PSK-CCMP]\tMy WiFi\nc\t2412\t-30\t[ESS]\tCafe\nd\t2412\t-20\t[WPA2-EAP-CCMP]\tWork\n","header\n7\tMy WiFi\tany\t[CURRENT]\n");assert_eq!(n.len(),3);assert_eq!(n[0].security,Security::Unsupported);assert_eq!(n[2].signal,-40);assert_eq!(n[2].saved,Some(7));}
    #[test]fn password_bytes(){assert!(psk("short").is_err());assert!(psk("1234567\n").is_err());assert_eq!(psk("Abc\"\\123").unwrap(),"\"Abc\"\\123\"");assert_eq!(psk(&"a".repeat(64)).unwrap(),"a".repeat(64));assert!(psk(&"z".repeat(64)).is_err());}
    #[test]fn helper_prefers_firmware_then_card(){
        let d=tempfile::tempdir().unwrap();let fw=d.path().join("wifi_init.sh");let root=d.path().join("Slot");
        assert_eq!(helper(&fw,&root),None);
        fs::create_dir_all(root.join("System")).unwrap();fs::write(root.join("System/brick_wifi.sh"),"").unwrap();
        assert_eq!(helper(&fw,&root),Some(root.join("System/brick_wifi.sh")));
        fs::write(&fw,"").unwrap();assert_eq!(helper(&fw,&root),Some(fw.clone()));
    }
    #[test]fn all_keys(){for s in KEYS{assert_eq!(s.len(),40)}for b in 32..=126{assert!(KEYS.iter().any(|s|s.as_bytes().contains(&b)),"{b}");}}
    #[test]fn failed_setup_restores_previous_connection(){let m=Mock::new(vec![("LIST_NETWORKS",Ok("header\n3\tOld\tany\t[CURRENT]\n4\tOther\tany\t[DISABLED]")),("ADD_NETWORK",Ok("8")),("SET_NETWORK 8 ssid 54657374",Err("rejected".into())),("REMOVE_NETWORK 8",Ok("OK")),("SELECT_NETWORK 3",Ok("OK")),("ENABLE_NETWORK 3",Ok("OK")),("DISABLE_NETWORK 4",Ok("OK"))]);assert!(connect(&m,&network(),Some("password".into()),&AtomicBool::new(false)).is_err());m.done();}
    #[test]fn cancelled_connect_removes_unsaved_profile(){let m=Mock::new(vec![("LIST_NETWORKS",Ok("header")),("ADD_NETWORK",Ok("8")),("SET_NETWORK 8 ssid 54657374",Ok("OK")),("SET_NETWORK 8 key_mgmt WPA-PSK",Ok("OK")),("SET_NETWORK 8 psk \"password\"",Ok("OK")),("SELECT_NETWORK 8",Ok("OK")),("REMOVE_NETWORK 8",Ok("OK")),("DISCONNECT",Ok("OK"))]);assert_eq!(connect(&m,&network(),Some("password".into()),&AtomicBool::new(true)).unwrap_err(),"Cancelled");m.done();}
    #[test]fn successful_connect_saves_only_after_association(){let m=Mock::new(vec![("LIST_NETWORKS",Ok("header")),("ADD_NETWORK",Ok("8")),("SET_NETWORK 8 ssid 54657374",Ok("OK")),("SET_NETWORK 8 key_mgmt WPA-PSK",Ok("OK")),("SET_NETWORK 8 psk \"password\"",Ok("OK")),("SELECT_NETWORK 8",Ok("OK")),("STATUS",Ok("id=8\nwpa_state=COMPLETED\nip_address=192.168.1.2")),("SAVE_CONFIG",Ok("OK"))]);assert_eq!(connect(&m,&network(),Some("password".into()),&AtomicBool::new(false)).unwrap(),"Connected; network saved");m.done();}
    #[test]#[ignore="Requires a Linux runtime allowing AF_UNIX datagram sockets"]fn socket_roundtrip(){let dir=tempfile::tempdir().unwrap();let remote=dir.path().join("server");let server=UnixDatagram::bind(&remote).unwrap();let t=thread::spawn(move||{let mut b=[0;64];let(n,peer)=server.recv_from(&mut b).unwrap();assert_eq!(&b[..n],b"PING");server.send_to(b"PONG\n",peer.as_pathname().unwrap()).unwrap();});let c=Control::at(&remote).unwrap();let local=c.local.clone();assert_eq!(fs::metadata(&local).unwrap().permissions().mode()&0o777,0o600);assert_eq!(c.command("PING").unwrap(),"PONG");drop(c);assert!(!local.exists());t.join().unwrap();}
    #[test]fn render_fixture(){let s=WifiMenu{worker:None,face:None,dirty:true,on:true,status:"Password is case sensitive".into(),networks:Vec::new(),row:0,keyboard:Some(Keyboard{network:network(),password:"Test".into(),key:0,page:0}),refreshed:Instant::now()};let f=s.render();assert_eq!(f.rgba.len(),640*480*4);if let Ok(path)=std::env::var("SLOT_WIFI_PREVIEW"){let mut e=png::Encoder::new(fs::File::create(path).unwrap(),640,480);e.set_color(png::ColorType::Rgba);e.set_depth(png::BitDepth::Eight);e.write_header().unwrap().write_image_data(&f.rgba).unwrap();}}
}
