//! CLI argument handling.

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;

use crate::geometry::Position;

#[derive(Parser)]
#[clap(version)]
pub struct Options {
    /// Background color.
    #[clap(short, long, value_name = "RRGGBB", default_value = "#000000")]
    pub color: Rgb,
    /// Background image.
    #[clap(short, long, value_name = "PATH")]
    pub image: Option<PathBuf>,
    /// Relative focus point; overflow is distributed evenly around this
    /// location.
    #[clap(short, long, value_name = "POINT", default_value = "0.5+0.5")]
    pub focus: Position<f32>,
}

/// RGB color.
#[derive(Copy, Clone)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl FromStr for Rgb {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Strip optional hash prefix.
        let color = s.strip_prefix('#').unwrap_or(s);

        // Ensure correct length.
        if color.len() != 6 {
            return Err("must contain exactly 6 hex digits");
        }

        // Parse all digits
        let combined = match u32::from_str_radix(color, 16) {
            Ok(combined) => combined,
            Err(_) => return Err("must only contain the characters 0-9 and a-f"),
        };

        Ok(Self {
            r: (combined >> 16) as u8,
            g: ((combined >> 8) & 255) as u8,
            b: (combined & 255) as u8,
        })
    }
}
