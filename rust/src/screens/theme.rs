//! Everforest theme constants. Mirrors `src/colors.ts` so the ratatui UI
//! and the TS Ink UI render against the same palette — handy when running
//! both binaries on the same workstation back-to-back.

use ratatui::style::Color;

pub const BG_PRIMARY: Color = Color::Rgb(0x2D, 0x35, 0x3B);
pub const BG_SECONDARY: Color = Color::Rgb(0x34, 0x3F, 0x44);

pub const FG_PRIMARY: Color = Color::Rgb(0xD3, 0xC6, 0xAA);
pub const FG_SECONDARY: Color = Color::Rgb(0x9D, 0xA9, 0xA0);

pub const RED: Color = Color::Rgb(0xE6, 0x7E, 0x80);
pub const ORANGE: Color = Color::Rgb(0xE6, 0x98, 0x75);
pub const YELLOW: Color = Color::Rgb(0xDB, 0xBC, 0x7F);
pub const GREEN: Color = Color::Rgb(0xA7, 0xC0, 0x80);
pub const AQUA: Color = Color::Rgb(0x83, 0xC0, 0x92);
pub const BLUE: Color = Color::Rgb(0x7F, 0xBB, 0xB3);
pub const PURPLE: Color = Color::Rgb(0xD6, 0x99, 0xB6);
