//! Theme tokens (dark), from `crates/ui/assets/theme.css`.

pub const BG_RAIL: u32 = 0x0d0f14;
pub const BG_SIDEBAR: u32 = 0x141720;
pub const BG_CHAT: u32 = 0x1a1e29;
pub const BG_INPUT: u32 = 0x0f1118;
pub const TEXT: u32 = 0xe6e8ee;
pub const TEXT_MUTED: u32 = 0x8a90a2;
pub const TEXT_FAINT: u32 = 0x5c6274;
pub const ACCENT: u32 = 0x7c8cff;
pub const ACCENT_STRONG: u32 = 0x5f72ff;
pub const DANGER: u32 = 0xff6b6b;
pub const RADIUS: f32 = 8.0;
/// `html { font-size: 15px }`: what 1rem is.
pub const REM: f32 = 15.0;

pub const BG_ELEVATED: u32 = 0x222736;
/// `rgba(255, 255, 255, 0.04)` and `0.08`, as 0xRRGGBBAA.
pub const BG_HOVER: u32 = 0xffffff0a;
pub const BG_SELECTED: u32 = 0xffffff14;
/// `--green`: the New Message tab.
pub const GREEN: u32 = 0x23a559;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_XL: f32 = 16.0;
pub const RAIL_W: f32 = 72.0;
pub const SIDEBAR_W: f32 = 248.0;
pub const BOTTOM_H: f32 = 50.0;
pub const BOTTOM_GAP: f32 = 12.0;
/// `--own`: the colour of your own name.
pub const OWN: u32 = 0xb8c0ff;
/// `.body a`: links in messages.
pub const LINK: u32 = 0x8fa0ff;
/// `rgba(124, 140, 255, 0.15)`, as 0xRRGGBBAA: a reaction of yours, a jumped-to message.
pub const ACCENT_TINT: u32 = 0x7c8cff26;
/// `--mono`.
pub const MONO: &str = "JetBrains Mono";
/// What body text is: Inter at `html { font-size: 15px; line-height: 1.4 }`.
pub const LINE_HEIGHT: f32 = 1.4;
pub const MEMBERS_W: f32 = 232.0;
