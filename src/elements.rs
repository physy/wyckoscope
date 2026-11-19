use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

/// A named palette for the default element colours.  Radii remain independent
/// from the palette so changing a scheme never changes the geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementColorScheme {
    Wyckoscope,
    Jmol,
    Cpk,
    Crystallographic,
    Rasmol,
    Colorblind,
}

impl ElementColorScheme {
    pub const ALL: [Self; 6] = [
        Self::Wyckoscope,
        Self::Jmol,
        Self::Cpk,
        Self::Crystallographic,
        Self::Rasmol,
        Self::Colorblind,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Wyckoscope => "Wyckoscope",
            Self::Jmol => "Jmol",
            Self::Cpk => "CPK",
            Self::Crystallographic => "Crystallographic Pastel",
            Self::Rasmol => "RasMol",
            Self::Colorblind => "Colorblind-friendly",
        }
    }
}

impl Default for ElementColorScheme {
    fn default() -> Self {
        Self::Crystallographic
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ElementStyle {
    pub color: Color,
    pub display_radius: f32,
    pub covalent_radius: f32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

pub fn style(symbol: &str) -> ElementStyle {
    let (color, display_radius, covalent_radius) = match symbol {
        "H" => (rgb(245, 247, 250), 0.28, 0.31),
        "He" => (rgb(217, 255, 255), 0.32, 0.28),
        "Li" => (rgb(204, 128, 255), 0.58, 1.28),
        "Be" => (rgb(194, 255, 0), 0.48, 0.96),
        "B" => (rgb(255, 181, 181), 0.42, 0.84),
        "C" => (rgb(55, 62, 72), 0.42, 0.76),
        "N" => (rgb(71, 111, 255), 0.40, 0.71),
        "O" => (rgb(255, 68, 68), 0.39, 0.66),
        "F" => (rgb(123, 234, 111), 0.37, 0.57),
        "Ne" => (rgb(179, 227, 245), 0.38, 0.58),
        "Na" => (rgb(171, 92, 242), 0.64, 1.66),
        "Mg" => (rgb(138, 255, 0), 0.58, 1.41),
        "Al" => (rgb(191, 166, 166), 0.55, 1.21),
        "Si" => (rgb(241, 190, 116), 0.53, 1.11),
        "P" => (rgb(255, 128, 0), 0.51, 1.07),
        "S" => (rgb(255, 220, 46), 0.51, 1.05),
        "Cl" => (rgb(47, 214, 62), 0.53, 1.02),
        "Ar" => (rgb(128, 209, 227), 0.48, 1.06),
        "K" => (rgb(143, 64, 212), 0.73, 2.03),
        "Ca" => (rgb(61, 255, 0), 0.69, 1.76),
        "Sc" => (rgb(230, 230, 230), 0.61, 1.70),
        "Ti" => (rgb(166, 171, 178), 0.59, 1.60),
        "V" => (rgb(166, 166, 171), 0.58, 1.53),
        "Cr" => (rgb(138, 153, 199), 0.57, 1.39),
        "Mn" => (rgb(156, 122, 199), 0.57, 1.39),
        "Fe" => (rgb(224, 102, 51), 0.58, 1.32),
        "Co" => (rgb(240, 144, 160), 0.57, 1.26),
        "Ni" => (rgb(80, 208, 80), 0.57, 1.24),
        "Cu" => (rgb(200, 117, 51), 0.58, 1.32),
        "Zn" => (rgb(125, 128, 176), 0.58, 1.22),
        "Ga" => (rgb(194, 143, 143), 0.58, 1.22),
        "Ge" => (rgb(102, 143, 143), 0.55, 1.20),
        "As" => (rgb(189, 128, 227), 0.55, 1.19),
        "Se" => (rgb(255, 161, 0), 0.55, 1.20),
        "Br" => (rgb(166, 41, 41), 0.55, 1.20),
        "Kr" => (rgb(92, 184, 209), 0.52, 1.16),
        "Rb" => (rgb(112, 46, 176), 0.76, 2.20),
        "Sr" => (rgb(0, 255, 0), 0.72, 1.95),
        "Y" => (rgb(148, 255, 255), 0.66, 1.90),
        "Zr" => (rgb(148, 224, 224), 0.64, 1.75),
        "Nb" => (rgb(115, 194, 201), 0.62, 1.64),
        "Mo" => (rgb(84, 181, 181), 0.61, 1.54),
        "Ru" => (rgb(36, 143, 143), 0.60, 1.46),
        "Rh" => (rgb(10, 125, 140), 0.60, 1.42),
        "Pd" => (rgb(0, 105, 133), 0.60, 1.39),
        "Ag" => (rgb(192, 192, 192), 0.61, 1.45),
        "Cd" => (rgb(255, 217, 143), 0.62, 1.44),
        "In" => (rgb(166, 117, 115), 0.62, 1.42),
        "Sn" => (rgb(102, 128, 128), 0.61, 1.39),
        "Sb" => (rgb(158, 99, 181), 0.60, 1.39),
        "Te" => (rgb(212, 122, 0), 0.59, 1.38),
        "I" => (rgb(148, 0, 148), 0.59, 1.39),
        "Cs" => (rgb(87, 23, 143), 0.80, 2.44),
        "Ba" => (rgb(0, 201, 0), 0.76, 2.15),
        "La" => (rgb(112, 212, 255), 0.70, 2.07),
        "Ce" => (rgb(255, 255, 199), 0.69, 2.04),
        "W" => (rgb(33, 148, 214), 0.63, 1.62),
        "Pt" => (rgb(208, 208, 224), 0.61, 1.36),
        "Au" => (rgb(255, 209, 35), 0.62, 1.36),
        "Hg" => (rgb(184, 184, 208), 0.63, 1.32),
        "Pb" => (rgb(87, 89, 97), 0.65, 1.46),
        "Bi" => (rgb(158, 79, 181), 0.65, 1.48),
        _ => (rgb(255, 90, 196), 0.52, 0.90),
    };

    ElementStyle {
        color,
        display_radius,
        covalent_radius,
    }
}

/// Resolves the base style for an element in a named colour scheme.
///
/// The historic Wyckoscope palette is deliberately the fallback for elements
/// not explicitly represented by a legacy palette. This retains useful,
/// distinctive colours for the full periodic table while the common elements
/// follow each selected convention.
pub fn style_for_scheme(symbol: &str, scheme: ElementColorScheme) -> ElementStyle {
    let mut base = style(symbol);
    base.color = palette_color(symbol, scheme, base.color);
    base
}

fn palette_color(symbol: &str, scheme: ElementColorScheme, fallback: Color) -> Color {
    let color = match scheme {
        ElementColorScheme::Wyckoscope => return fallback,
        // Jmol's CPK-derived palette is the familiar convention in many
        // molecular viewers.
        ElementColorScheme::Jmol => match symbol {
            "H" => (255, 255, 255),
            "He" => (217, 255, 255),
            "Li" => (204, 128, 255),
            "Be" => (194, 255, 0),
            "B" => (255, 181, 181),
            "C" => (144, 144, 144),
            "N" => (48, 80, 248),
            "O" => (255, 13, 13),
            "F" | "Cl" => (144, 224, 80),
            "Ne" | "Ar" | "Kr" | "Xe" => (128, 209, 227),
            "Na" => (171, 92, 242),
            "Mg" | "Ca" | "Sr" | "Ba" => (61, 255, 0),
            "Al" => (191, 166, 166),
            "Si" => (240, 200, 160),
            "P" => (255, 128, 0),
            "S" => (255, 255, 48),
            "K" | "Rb" | "Cs" => (143, 64, 212),
            "Ti" => (191, 194, 199),
            "Cr" => (138, 153, 199),
            "Mn" => (156, 122, 199),
            "Fe" => (224, 102, 51),
            "Co" => (240, 144, 160),
            "Ni" => (80, 208, 80),
            "Cu" => (200, 128, 51),
            "Zn" => (125, 128, 176),
            "Ga" => (194, 143, 143),
            "Ge" => (102, 143, 143),
            "As" => (189, 128, 227),
            "Se" => (255, 161, 0),
            "Br" => (166, 41, 41),
            "Ag" => (192, 192, 192),
            "Cd" => (255, 217, 143),
            "Sn" => (102, 128, 128),
            "I" => (148, 0, 148),
            "Au" => (255, 209, 35),
            "Hg" => (184, 184, 208),
            "Pb" => (87, 89, 97),
            _ => return fallback,
        },
        // The traditional CPK set intentionally has high-contrast, simple
        // colours. It is useful for figures that need a classic appearance.
        ElementColorScheme::Cpk => match symbol {
            "H" => (255, 255, 255),
            "C" => (55, 55, 55),
            "N" => (48, 80, 248),
            "O" => (240, 0, 0),
            "F" | "Cl" => (0, 190, 0),
            "Br" => (130, 35, 35),
            "I" => (100, 0, 180),
            "P" => (255, 140, 0),
            "S" => (255, 220, 0),
            "B" => (255, 170, 170),
            "Si" => (220, 180, 120),
            "Fe" => (210, 110, 50),
            "Cu" => (190, 110, 45),
            "Zn" => (125, 128, 176),
            "Ag" => (190, 190, 190),
            "Au" => (255, 200, 0),
            "Hg" => (170, 170, 190),
            _ => return fallback,
        },
        // An independently designed, low-saturation palette for crystal
        // figures. It deliberately does not reuse VESTA's distributed table.
        ElementColorScheme::Crystallographic => match symbol {
            "H" => (248, 205, 207),
            "D" => (193, 218, 248),
            "He" => (244, 222, 193),
            "Li" => (131, 202, 132),
            "Be" => (102, 187, 157),
            "B" => (91, 157, 88),
            "C" => (137, 91, 68),
            "N" => (144, 174, 224),
            "O" => (238, 67, 59),
            "F" => (139, 193, 222),
            "Ne" => (228, 134, 185),
            "Na" => (224, 198, 84),
            "Mg" => (231, 142, 83),
            "Al" => (128, 172, 207),
            "Si" => (80, 118, 205),
            "P" => (187, 145, 191),
            "S" => (245, 219, 58),
            "Cl" => (93, 207, 87),
            "Ar" => (196, 229, 178),
            "K" => (164, 94, 215),
            "Ca" => (106, 157, 198),
            "Sc" => (184, 113, 183),
            "Ti" => (105, 184, 224),
            "V" => (212, 85, 77),
            "Cr" => (85, 99, 176),
            "Mn" => (177, 80, 163),
            "Fe" => (193, 126, 64),
            "Co" => (91, 91, 182),
            "Ni" => (130, 155, 154),
            "Cu" => (82, 124, 204),
            "Zn" => (151, 151, 139),
            "Ga" => (151, 203, 106),
            "Ge" => (142, 119, 181),
            "As" => (117, 194, 102),
            "Se" => (182, 224, 72),
            "Br" => (154, 80, 45),
            "Kr" => (230, 164, 210),
            "Rb" => (216, 86, 166),
            "Sr" => (99, 208, 117),
            "Y" => (116, 163, 157),
            "Zr" => (75, 191, 168),
            "Nb" => (104, 173, 132),
            "Mo" => (168, 131, 176),
            "Tc" => (194, 158, 189),
            "Ru" => (194, 168, 151),
            "Rh" => (190, 193, 151),
            "Pd" | "Ag" => (172, 181, 185),
            "Cd" => (223, 93, 202),
            "In" => (203, 140, 186),
            "Sn" => (167, 150, 190),
            "Sb" => (210, 139, 103),
            "Te" => (173, 160, 82),
            "I" => (145, 66, 138),
            "Xe" => (158, 165, 228),
            "Cs" => (72, 205, 178),
            "Ba" => (88, 202, 107),
            "La" => (107, 184, 97),
            "Ce" | "Pr" | "Nd" | "Sm" | "Eu" | "Gd" | "Tb" | "Dy" | "Ho" | "Er" | "Tm" | "Yb"
            | "Lu" => (211, 181, 67),
            "Hf" => (172, 171, 88),
            "Ta" => (181, 155, 83),
            "W" => (144, 140, 130),
            "Re" | "Os" | "Ir" | "Pt" => (190, 179, 141),
            "Au" => (244, 185, 67),
            "Hg" => (203, 175, 200),
            "Tl" => (160, 147, 119),
            "Pb" => (97, 99, 109),
            "Bi" => (195, 83, 210),
            "Po" | "At" => (105, 105, 194),
            "Rn" => (220, 206, 77),
            "Fr" => (72, 72, 76),
            "Ra" => (120, 181, 102),
            "Ac" => (109, 166, 124),
            "Th" => (91, 209, 128),
            "Pa" => (99, 198, 106),
            "U" => (112, 159, 170),
            "Np" | "Pu" | "Am" => (105, 105, 105),
            _ => return fallback,
        },
        ElementColorScheme::Rasmol => match symbol {
            "H" => (255, 255, 255),
            "C" => (200, 200, 200),
            "N" => (143, 143, 255),
            "O" => (240, 0, 0),
            "F" | "Cl" => (0, 255, 0),
            "Br" => (166, 41, 41),
            "I" => (160, 32, 240),
            "P" => (255, 165, 0),
            "S" => (255, 200, 50),
            "Fe" => (255, 165, 0),
            "Ca" => (128, 128, 144),
            "Zn" => (165, 42, 42),
            "Cu" => (165, 42, 42),
            "Ag" => (192, 192, 192),
            "Au" => (218, 165, 32),
            _ => return fallback,
        },
        // Okabe-Ito-derived hues avoid red/green-only distinctions. The
        // mapping is deliberately by element family so it remains legible for
        // structures with many species.
        ElementColorScheme::Colorblind => match symbol {
            "H" | "He" | "Ne" | "Ar" | "Kr" | "Xe" => (235, 235, 235),
            "C" | "Si" | "Ge" | "Sn" | "Pb" => (0, 114, 178),
            "N" | "P" | "As" | "Sb" | "Bi" => (86, 180, 233),
            "O" | "S" | "Se" | "Te" => (213, 94, 0),
            "F" | "Cl" | "Br" | "I" => (0, 158, 115),
            "B" | "Al" | "Ga" | "In" => (230, 159, 0),
            "Li" | "Na" | "K" | "Rb" | "Cs" => (204, 121, 167),
            "Be" | "Mg" | "Ca" | "Sr" | "Ba" => (240, 228, 66),
            "Ti" | "Zr" | "Hf" | "V" | "Nb" | "Ta" | "Cr" | "Mo" | "W" => (0, 114, 178),
            "Mn" | "Fe" | "Co" | "Ni" | "Cu" | "Zn" | "Ru" | "Rh" | "Pd" | "Ag" | "Pt" | "Au"
            | "Hg" => (204, 121, 167),
            _ => return fallback,
        },
    };
    rgb(color.0, color.1, color.2)
}

pub fn normalize_symbol(raw: &str) -> String {
    let letters: String = raw.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() {
        return "X".to_string();
    }
    let mut chars = letters.chars();
    let first = chars.next().unwrap().to_ascii_uppercase();
    let second = chars.next().filter(|c| c.is_ascii_lowercase());
    match second {
        Some(c) => format!("{first}{}", c.to_ascii_lowercase()),
        None => first.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::ColorToPacked;

    #[test]
    fn palettes_change_colour_without_changing_radii() {
        let base = style_for_scheme("C", ElementColorScheme::Wyckoscope);
        let jmol = style_for_scheme("C", ElementColorScheme::Jmol);

        assert_eq!(base.display_radius, jmol.display_radius);
        assert_eq!(base.covalent_radius, jmol.covalent_radius);
        assert_eq!(
            jmol.color.to_srgba().to_u8_array_no_alpha(),
            [144, 144, 144]
        );
        assert_ne!(
            base.color.to_srgba().to_u8_array_no_alpha(),
            jmol.color.to_srgba().to_u8_array_no_alpha()
        );
    }
}
