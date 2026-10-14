mod common;

use slot_retro::RetroCore;
use slot_store::Core;

fn dylib() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor")
        .join(slot::core::dylib_name(Core::Fceumm))
}

/// An NROM cart whose program paints the background white and loops.
fn white_screen_rom() -> Vec<u8> {
    let mut rom = b"NES\x1a\x01\x01\0\0\0\0\0\0\0\0\0\0".to_vec();
    let mut prg = vec![0u8; 0x4000];
    let program = [
        0xa9, 0x3f, 0x8d, 0x06, 0x20, // LDA #$3F; STA $2006
        0xa9, 0x00, 0x8d, 0x06, 0x20, // LDA #$00; STA $2006
        0xa9, 0x30, 0x8d, 0x07, 0x20, // LDA #$30; STA $2007: backdrop white
        0xa9, 0x00, 0x8d, 0x06, 0x20, 0x8d, 0x06, 0x20, // address back to $0000
        0xa9, 0x08, 0x8d, 0x01, 0x20, // LDA #$08; STA $2001: background on
        0x4c, 0x1c, 0xc0, // JMP $C01C
    ];
    prg[..program.len()].copy_from_slice(&program);
    for vector in [0x3ffa, 0x3ffc, 0x3ffe] {
        prg[vector..vector + 2].copy_from_slice(&[0x00, 0xc0]);
    }
    rom.extend(prg);
    rom.extend(vec![0u8; 0x2000]);
    rom
}

#[test]
fn a_nes_game_draws_its_picture_centred_on_the_nes_canvas() {
    let path = dylib();
    if !path.exists() {
        eprintln!("no fceumm dylib on this host, skipping");
        return;
    }
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("white.nes");
    std::fs::write(&rom, white_screen_rom()).unwrap();
    let mut core = slot::core::open_core_for(d.path(), Core::Fceumm, "auto", false, &[path]);
    core.load(&rom).expect("FCEUmm refused the cart");
    for _ in 0..10 {
        core.run_frame(slot_retro::ButtonMask(0));
    }
    let (w, h) = core.canvas();
    assert_eq!((w, h), slot_store::Platform::Nes.canvas());
    let video = core.video_xrgb8888();
    assert_eq!(video.len(), (w * h * 4) as usize);
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [video[i], video[i + 1], video[i + 2]]
    };
    assert!(px(128, 120).iter().all(|c| *c > 0xc0), "the picture is not white: {:?}", px(128, 120));
    assert!(px(128, 9).iter().all(|c| *c > 0xc0), "the first line of the picture is missing");
    assert_eq!(px(128, 3), [0, 0, 0], "the cropped overscan is not black above the picture");
    assert_eq!(px(128, 236), [0, 0, 0], "the cropped overscan is not black below the picture");

    let state = core.serialize().expect("no save state");
    core.run_frame(slot_retro::ButtonMask(0));
    core.unserialize(&state).expect("its own save state was refused");
    assert!(core.av_info().sample_rate > 0.0);
}

#[test]
fn every_option_slot_sets_on_fceumm_is_one_it_declares() {
    let path = dylib();
    if !path.exists() {
        eprintln!("no fceumm dylib on this host, skipping");
        return;
    }
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("white.nes");
    std::fs::write(&rom, white_screen_rom()).unwrap();
    let mut core = slot_retro::LibretroCore::open(&path).expect("open FCEUmm");
    slot::core::apply_core_options(&mut core, Core::Fceumm, "auto", false, true, None);
    // It declares its options as the game loads, and reads slot's values then.
    core.load(&rom).expect("FCEUmm refused the cart");
    let declared = core.declared_options();
    assert!(!declared.is_empty(), "FCEUmm declared no options, so this would pass on anything");
    let set = core.options();
    assert!(set.iter().any(|(k, _)| k == "fceumm_overscan_v_top"), "{set:?}");
    for (key, value) in set {
        let values = declared.get(&key).unwrap_or_else(|| panic!("FCEUmm declares no option {key:?}"));
        assert!(values.contains(&value), "FCEUmm declares {key:?} as {values:?}, not {value:?}");
    }
}

#[test]
fn the_emulator_thread_hands_the_screen_whole_nes_frames() {
    use slot::audio::{AudioSink, StubSink};
    use slot::emu::{CoreState, EmuHandle, Speed};
    let path = dylib();
    if !path.exists() {
        eprintln!("no fceumm dylib on this host, skipping");
        return;
    }
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("white.nes");
    std::fs::write(&rom, white_screen_rom()).unwrap();
    let core = slot::core::open_core_for(d.path(), Core::Fceumm, "auto", false, &[path]);
    let mut sink = StubSink::new();
    sink.open(32_768).unwrap();
    let emu = EmuHandle::spawn(core, rom, sink.ring(), None, None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while emu.state() != CoreState::Ready && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(emu.state(), CoreState::Ready, "FCEUmm never finished loading");
    emu.set_speed(Speed::Normal);
    while !emu.has_published() && std::time::Instant::now() < deadline {
        sink.device_drain();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let frame = emu.latest_frame().expect("no frame was published");
    let (w, h) = slot_store::Platform::Nes.canvas();
    assert_eq!(frame.len(), (w * h * 4) as usize, "a frame is not the NES canvas");
}
