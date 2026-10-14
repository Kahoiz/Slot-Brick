use std::path::Path;

use slot_gfx::WHOLE_TEXTURE;
#[cfg(all(test, feature = "brick"))]
use slot_gfx::{SRC_H, SRC_W};
use slot_store::{ini, Platform};

pub const VIDEO_MODE_FILE: &str = if cfg!(feature = "brick") {
    "Config/brick_video_mode_v3.ini"
} else {
    "Config/video_mode.ini"
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoMode {
    Actual,
    Stretch,
}

impl Default for VideoMode {
    fn default() -> Self { Self::Actual }
}

impl VideoMode {
    pub fn as_str(self) -> &'static str {
        match self {
            VideoMode::Actual => "actual",
            VideoMode::Stretch => "stretch",
        }
    }

    pub fn parse(s: &str) -> Option<VideoMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "actual" => Some(VideoMode::Actual),
            "stretch" => Some(VideoMode::Stretch),
            _ => None,
        }
    }
}

pub fn video_mode_for(root: &Path, stem: &str) -> VideoMode {
    video_mode_for_platform(root, stem, Platform::Gba)
}

pub fn default_for_platform(_platform: Platform) -> VideoMode {
    VideoMode::Actual
}

pub fn video_mode_for_platform(root: &Path, stem: &str, platform: Platform) -> VideoMode {
    ini::value(root, VIDEO_MODE_FILE, stem)
        .as_deref()
        .and_then(VideoMode::parse)
        .unwrap_or_else(|| default_for_platform(platform))
}

pub fn write_video_mode(root: &Path, stem: &str, mode: VideoMode) -> std::io::Result<()> {
    ini::write(root, VIDEO_MODE_FILE, stem, mode.as_str())
}

pub fn source_rect(platform: Platform, mode: VideoMode) -> [f32; 4] {
    let (w, h) = platform.picture();
    // In canvas units: a NES frame arrives on a 256x240 canvas, the Game Boys' on 240x160.
    let (src_w, src_h) = platform.canvas();
    #[cfg(feature = "brick")]
    if mode == VideoMode::Actual {
        // Expand the source rectangle to 4:3; the shader paints the region
        // outside the active picture black. Stretch mode below has no bars.
        let aspect = slot_gfx::OUT_W as f32 / slot_gfx::OUT_H as f32;
        let (vw, vh) = if w as f32 / h as f32 > aspect {
            (w as f32, w as f32 / aspect)
        } else {
            (h as f32 * aspect, h as f32)
        };
        return [
            (src_w as f32 - vw) / (2.0 * src_w as f32),
            (src_h as f32 - vh) / (2.0 * src_h as f32),
            vw / src_w as f32,
            vh / src_h as f32,
        ];
    }
    if mode == VideoMode::Actual || (w, h) == (src_w, src_h) {
        return WHOLE_TEXTURE;
    }
    let x = src_w.saturating_sub(w) / 2;
    let y = src_h.saturating_sub(h) / 2;
    [
        x as f32 / src_w as f32,
        y as f32 / src_h as f32,
        w as f32 / src_w as f32,
        h as f32 / src_h as f32,
    ]
}

#[cfg(all(test, feature = "brick"))]
mod brick_tests {
    use super::*;
    #[test]
    fn default_fills_screen_without_cropping_game_pixels() {
        assert_eq!(VideoMode::default(), VideoMode::Actual);
        assert_eq!((slot_gfx::OUT_W, slot_gfx::OUT_H), (640, 480));
        assert_eq!((SRC_W, SRC_H), (240, 160));
        for platform in [Platform::Gb, Platform::Gbc, Platform::Gba] {
            let [x, y, w, h] = source_rect(platform, VideoMode::Stretch);
            let (pw, ph) = platform.picture();
            assert!((w * SRC_W as f32 - pw as f32).abs() < 0.001);
            assert!((h * SRC_H as f32 - ph as f32).abs() < 0.001);
            assert!((2.0 * x + w - 1.0).abs() < 0.001);
            assert!((2.0 * y + h - 1.0).abs() < 0.001);
        }
    }
    #[test]
    fn defaults_are_platform_specific_and_saved_choices_work() {
        let root = tempfile::tempdir().unwrap();
        for platform in [Platform::Gb, Platform::Gbc] {
            assert_eq!(
                video_mode_for_platform(root.path(), "unsaved", platform),
                VideoMode::Actual
            );
            let [_, _, w, h] = source_rect(platform, default_for_platform(platform));
            let (pw, ph) = platform.picture();
            let ratio =
                (4.0 / 3.0) * (pw as f32 / (w * SRC_W as f32)) / (ph as f32 / (h * SRC_H as f32));
            assert!((ratio - 10.0 / 9.0).abs() < 0.001);
        }
        assert_eq!(
            video_mode_for_platform(root.path(), "unsaved", Platform::Gba),
            VideoMode::Actual
        );
        assert_eq!(
            source_rect(Platform::Gba, default_for_platform(Platform::Gba)),
            [0.0, -0.0625, 1.0, 1.125]
        );
        write_video_mode(root.path(), "choice", VideoMode::Actual).unwrap();
        assert_eq!(
            video_mode_for_platform(root.path(), "choice", Platform::Gba),
            VideoMode::Actual
        );
    }

    #[test]
    fn a_nes_picture_sits_in_its_own_canvas() {
        let (cw, ch) = Platform::Nes.canvas();
        let [x, y, w, h] = source_rect(Platform::Nes, VideoMode::Stretch);
        assert_eq!((w * cw as f32, h * ch as f32), (256.0, 224.0), "the overscan-cropped picture");
        assert_eq!((x, y * ch as f32), (0.0, 8.0), "centred, 8 lines down");
        let [_, _, w, h] = source_rect(Platform::Nes, VideoMode::Actual);
        assert!((w * cw as f32 / (h * ch as f32) - 4.0 / 3.0).abs() < 0.001, "padded to 4:3");
        assert!((h * ch as f32 - 224.0).abs() < 0.001, "no line cropped");
    }

    #[test]
    fn actual_mode_preserves_original_aspect() {
        for platform in [Platform::Gb, Platform::Gbc, Platform::Gba] {
            let [_, _, w, h] = source_rect(platform, VideoMode::Actual);
            assert!((w * SRC_W as f32 / (h * SRC_H as f32) - 4.0 / 3.0).abs() < 0.001);
        }
    }
}
