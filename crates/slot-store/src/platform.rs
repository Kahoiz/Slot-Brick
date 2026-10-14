use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
    Nes,
}

impl Platform {
    pub const ALL: [Platform; 4] = [Platform::Gba, Platform::Gb, Platform::Gbc, Platform::Nes];

    pub fn name(self) -> &'static str {
        match self {
            Platform::Gba => "Game Boy Advance",
            Platform::Gb => "Game Boy",
            Platform::Gbc => "Game Boy Color",
            Platform::Nes => "Nintendo Entertainment System",
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
            Platform::Nes => "NES",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Platform::Gba => &["gba"],
            Platform::Gb | Platform::Gbc => &["gb", "gbc"],
            Platform::Nes => &["nes"],
        }
    }

    pub fn picture(self) -> (u32, u32) {
        match self {
            Platform::Gba => (240, 160),
            Platform::Gb | Platform::Gbc => (160, 144),
            // With FCEUmm's default 8-line overscan crop top and bottom.
            Platform::Nes => (256, 224),
        }
    }

    /// The frame canvas a core draws into, its picture centred: GBA-sized for the Game
    /// Boys, a NES's full 256x240 (overscan included) for the NES.
    pub fn canvas(self) -> (u32, u32) {
        match self {
            Platform::Gba | Platform::Gb | Platform::Gbc => (240, 160),
            Platform::Nes => (256, 240),
        }
    }

    pub fn accepts(self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|k| ext.eq_ignore_ascii_case(k))
            })
    }
}
