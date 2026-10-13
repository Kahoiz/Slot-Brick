#![cfg(feature = "brick")]
#[test]
fn slot_art_keeps_cartridge_opening_clear_and_front_opaque() {
    let back=slot_ui::slot_art_face(false).unwrap();
    let front=slot_ui::slot_art_face(true).unwrap();
    assert_eq!((front.w,front.h),(1280,116));
    assert_eq!((back.w,back.h),(front.w,front.h));
    let alpha=|x:u32,y:u32|front.rgba[((y*front.w+x)*4+3) as usize];
    assert_eq!(alpha(640,30),0); // cartridge remains visible inside the mouth
    assert_eq!(alpha(640,115),255); // foreground hides the inserted cartridge
    assert!(front.rgba.chunks_exact(4).any(|p|p[3]>0&&p[3]<255));
    if let Ok(dir)=std::env::var("SLOT_ART_REVIEW") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name,f) in [("back.png",back),("front.png",front)] {
            let mut e=png::Encoder::new(std::fs::File::create(std::path::Path::new(&dir).join(name)).unwrap(),f.w,f.h);
            e.set_color(png::ColorType::Rgba);e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&f.rgba).unwrap();
        }
    }
}
