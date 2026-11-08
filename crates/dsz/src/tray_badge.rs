//! Pixel-aligned battery digits that stay legible at notification-area sizes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode { #[default] Off, Static, Battery }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Display { #[default] Overlay, Replace, Separate }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub mode: ColorMode,
    pub color: [u8; 3],
    pub last_mode: ColorMode,
    pub display: Display,
}

impl Default for Style {
    fn default() -> Self { Self { mode: ColorMode::Off, color: [45, 127, 249], last_mode: ColorMode::Static, display: Display::Overlay } }
}

impl Style {
    pub fn set_mode(&mut self, mode: ColorMode) {
        if self.mode != ColorMode::Off { self.last_mode = self.mode; }
        self.mode = mode;
        if mode != ColorMode::Off { self.last_mode = mode; }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.set_mode(if enabled {
            if self.last_mode == ColorMode::Off { ColorMode::Static } else { self.last_mode }
        } else { ColorMode::Off });
    }
}

pub fn appearance(style: &Style, battery: Option<u8>) -> (Badge, [u8; 3]) {
    if style.mode == ColorMode::Off { return (Badge::Off, [0; 3]); }
    let color = match (style.mode, battery) {
        (ColorMode::Battery, Some(n)) => crate::composer::battery_color(n.min(100)),
        (ColorMode::Battery, None) => [164, 174, 189],
        _ => style.color,
    };
    (battery.map_or(Badge::Disconnected, |n| Badge::Level(n.min(100))), color)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPlan {
    pub main: (Badge, [u8; 3], bool),
    pub separate: Option<(Badge, [u8; 3])>,
}

pub fn display_plan(style: &Style, battery: Option<u8>) -> DisplayPlan {
    let plain = (Badge::Off, [0; 3], false);
    let (badge, color) = appearance(style, battery);
    if badge == Badge::Off { return DisplayPlan { main: plain, separate: None }; }
    match style.display {
        Display::Overlay => DisplayPlan { main: (badge, color, false), separate: None },
        Display::Replace => DisplayPlan { main: if battery.is_some() { (badge, color, true) } else { plain }, separate: None },
        Display::Separate => DisplayPlan { main: plain, separate: battery.map(|_| (badge, color)) },
    }
}

fn luminance(color: [u8; 3]) -> f64 {
    let linear = |c: u8| { let c = c as f64 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
    0.2126 * linear(color[0]) + 0.7152 * linear(color[1]) + 0.0722 * linear(color[2])
}

pub fn background(color: [u8; 3]) -> [u8; 3] {
    let l = luminance(color);
    if (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) { [0; 3] } else { [255; 3] }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge { Off, Disconnected, Level(u8) }

const DIGITS: [[u8; 7]; 10] = [
    [7,5,5,5,5,5,7], [2,6,2,2,2,2,7], [7,1,1,7,4,4,7], [7,1,1,7,1,1,7], [5,5,5,7,1,1,1],
    [7,4,4,7,1,1,7], [7,4,4,7,5,5,7], [7,1,1,2,2,2,2], [7,5,5,7,5,5,7], [7,5,5,7,1,1,7],
];

#[cfg(any(test, debug_assertions))]
pub fn paint(size: usize, rgba: &mut [u8], badge: Badge, color: [u8; 3]) {
    draw(size, rgba, badge, color, false);
}

pub fn render(size: usize, badge: Badge, color: [u8; 3], battery_only: bool) -> Vec<u8> {
    let mut rgba = crate::icon_art::icon_rgba(size);
    if battery_only && !matches!(badge, Badge::Level(_)) { return rgba; }
    draw(size, &mut rgba, badge, color, battery_only);
    rgba
}

fn draw(size: usize, rgba: &mut [u8], badge: Badge, color: [u8; 3], battery_only: bool) {
    if badge == Badge::Off || size < 12 || rgba.len() != size * size * 4 { return; }
    let text = match badge { Badge::Level(n) => n.min(100).to_string(), _ => "-".into() };
    let bg = background(color);
    let color = [color[0], color[1], color[2], 255];
    let margin = (size / 16).max(1);
    let columns = text.len() * 4 - 1;
    let height = if battery_only { size - 2 * margin } else { (size * 2 / 3).max(7).min(size - 2 * margin) };
    let width = (columns * height).div_ceil(7).min(size - 2 * margin);
    let left = if battery_only { (size - width) / 2 } else { size - width - margin };
    let top = margin;
    let pixel = |rgba: &mut [u8], x: usize, y: usize, color: [u8; 4]| {
        if x < size && y < size { rgba[(y * size + x) * 4..(y * size + x) * 4 + 4].copy_from_slice(&color); }
    };
    let bg_left = if battery_only { 0 } else { left - margin };
    let bg_height = if battery_only { size } else { height + 2 * margin };
    for y in 0..bg_height { for x in bg_left..size { pixel(rgba, x, y, [bg[0], bg[1], bg[2], 255]); } }
    // Scale the glyph grid to the available pixels, including non-integral
    // scales at 20/24 px and three digits at 100%, without cropping digits.
    let chars: Vec<u8> = text.bytes().collect();
    for y in 0..height { for x in 0..width {
        let col = x * columns / width;
        let glyph = if chars[col / 4] == b'-' { [0,0,0,7,0,0,0] } else { DIGITS[(chars[col / 4] - b'0') as usize] };
        if col % 4 < 3 && glyph[y * 7 / height] & (1 << (2 - col % 4)) != 0 {
            pixel(rgba, left + x, top + y, color);
        }
    } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_badge_uses_the_chosen_text_color_and_off_disables_it() {
        let style = Style { mode: ColorMode::Static, color: [255, 0, 0], ..Default::default() };
        assert_eq!(appearance(&style, Some(80)), (Badge::Level(80), [255, 0, 0]));
        assert_eq!(appearance(&style, None), (Badge::Disconnected, [255, 0, 0]));
        assert_eq!(appearance(&Style::default(), Some(80)).0, Badge::Off);
        assert_eq!(background([0, 0, 0]), [255; 3]);
        assert_eq!(background([255; 3]), [0; 3]);
    }

    #[test]
    fn battery_badge_uses_the_exact_lightbar_gradient() {
        let style = Style { mode: ColorMode::Battery, ..Default::default() };
        for percent in [0, 10, 20, 50, 75, 100] {
            assert_eq!(appearance(&style, Some(percent)), (Badge::Level(percent), crate::composer::battery_color(percent)));
        }
    }
    #[test]
    fn battery_only_fills_the_icon_and_restores_dsz_when_disconnected_or_off() {
        for size in [16, 20, 24, 32] {
            let base = crate::icon_art::icon_rgba(size);
            assert_eq!(render(size, Badge::Disconnected, [255,0,0], true), base);
            assert_eq!(render(size, Badge::Off, [255,0,0], true), base);
            for n in [0, 9, 80, 100] {
                let image = render(size, Badge::Level(n), [255,0,0], true);
                assert!(image.chunks_exact(4).all(|p| p == [0,0,0,255] || p == [255,0,0,255]));
                let rows: Vec<_> = image.chunks_exact(size * 4).enumerate()
                    .filter(|(_, row)| row.chunks_exact(4).any(|p| p == [255,0,0,255])).map(|(y,_)| y).collect();
                assert_eq!(rows.last().unwrap() - rows.first().unwrap() + 1, size - 2 * (size / 16).max(1));
            }
        }
    }
    #[test]
    fn disabled_badge_preserves_icon_and_disconnect_clears_the_old_number() {
        for size in [16, 20, 24, 32, 48, 64] {
            let base = crate::icon_art::icon_rgba(size);
            let mut off = base.clone(); paint(size, &mut off, Badge::Off, [0; 3]); assert_eq!(off, base);
            let mut level = base.clone(); paint(size, &mut level, Badge::Level(80), [255, 0, 0]); assert_ne!(level, base);
            let mut absent = base.clone(); paint(size, &mut absent, Badge::Disconnected, [255, 0, 0]); assert_ne!(level, absent);
            assert_eq!(&level[(size - 1) * size * 4..], &base[(size - 1) * size * 4..], "badge stays above the bottom row");
            for n in [0, 9, 20, 50, 99, 100, 255] {
                let mut image = base.clone(); paint(size, &mut image, Badge::Level(n), [0, 0, 255]);
                assert_eq!(image.len(), size * size * 4);
            }
        }
        let profile: crate::profile::Profile = serde_json::from_str("{}").unwrap();
        assert_eq!(profile.lighting.tray_badge.mode, ColorMode::Off);
    }

    #[test]
    fn settings_shortcut_and_lighting_mode_share_state_and_restore_battery() {
        let mut style = Style::default();
        style.set_enabled(true); assert_eq!(style.mode, ColorMode::Static);
        style.set_mode(ColorMode::Battery);
        style.set_enabled(false); assert_eq!(style.mode, ColorMode::Off);
        style.set_enabled(true); assert_eq!(style.mode, ColorMode::Battery);
        style.set_mode(ColorMode::Off);
        style.set_enabled(true); assert_eq!(style.mode, ColorMode::Battery);
    }

    #[test]
    fn text_pixels_keep_the_selected_color_over_an_opaque_contrasting_plate() {
        for color in [[0,0,0], [255,255,255], [120,120,120], [45,127,249], [255,0,0]] {
            let bg = background(color);
            let l = luminance(color); let b = luminance(bg);
            assert!((l.max(b) + 0.05) / (l.min(b) + 0.05) >= 4.5);
            let mut image = vec![255; 16 * 16 * 4];
            paint(16, &mut image, Badge::Level(80), color);
            assert!(image.chunks_exact(4).any(|p| p == [color[0],color[1],color[2],255]));
            let left = 16 - 9;
            assert_eq!(&image[left * 4..left * 4 + 4], &[bg[0],bg[1],bg[2],255]);
        }
    }

    #[test]
    fn display_choices_control_both_icons_without_leaving_a_disconnected_battery_icon() {
        let mut style = Style { mode: ColorMode::Static, ..Default::default() };
        assert_eq!(display_plan(&style, Some(80)).main, (Badge::Level(80), style.color, false));
        style.display = Display::Replace;
        assert_eq!(display_plan(&style, Some(80)).main, (Badge::Level(80), style.color, true));
        assert_eq!(display_plan(&style, None).main.0, Badge::Off);
        style.display = Display::Separate;
        let p = display_plan(&style, Some(80));
        assert_eq!(p.main.0, Badge::Off);
        assert_eq!(p.separate, Some((Badge::Level(80), style.color)));
        assert_eq!(display_plan(&style, None).separate, None);
        style.set_enabled(false);
        assert_eq!(display_plan(&style, Some(80)).separate, None);
        style.set_enabled(true);
        assert_eq!(style.display, Display::Separate);
        assert!(display_plan(&style, Some(80)).separate.is_some());
    }
}
