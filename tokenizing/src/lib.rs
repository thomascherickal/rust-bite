//! IBM inspired colors currently used by the GUI.

pub mod colors {
    use super::Color;

    // necessary as floating-point const function aren't stable yet
    #[macro_export]
    macro_rules! color {
        ($r:expr, $g:expr, $b:expr) => {
            $crate::Color([$r as f32 / 255.0, $g as f32 / 255.0, $b as f32 / 255.0, 1.0])
        };
    }

    pub const WHITE: Color = color!(0xff, 0xff, 0xff);
    pub const BLUE: Color = color!(0x0f, 0x62, 0xfe);
    pub const MAGENTA: Color = color!(0xf5, 0x12, 0x81);
    pub const RED: Color = color!(0xff, 0x00, 0x0b);
    pub const PURPLE: Color = color!(0xc4, 0x91, 0xfd);
    pub const GRAY10: Color = color!(0x10, 0x10, 0x10);
    pub const GRAY20: Color = color!(0x20, 0x20, 0x20);
    pub const GRAY40: Color = color!(0x40, 0x40, 0x40);
    pub const GRAY99: Color = color!(0x99, 0x99, 0x99);
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Color([f32; 4]);

unsafe impl bytemuck::Zeroable for Color {}
unsafe impl bytemuck::Pod for Color {}

impl From<Color> for [f32; 4] {
    fn from(val: Color) -> Self {
        val.0
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub text: std::borrow::Cow<'static, str>,
    pub color: Color,
}

impl Token {
    pub fn text(&self, scale: f32) -> wgpu_glyph::Text {
        wgpu_glyph::Text::new(&self.text)
            .with_color(self.color)
            .with_scale(scale)
    }
}

pub const EMPTY_TOKEN: Token = Token {
    color: colors::WHITE,
    text: std::borrow::Cow::Borrowed(""),
};
