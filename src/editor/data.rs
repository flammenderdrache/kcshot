use gtk4::{gdk::RGBA, pango::FontDescription};

mod point;
pub use point::Point;

#[derive(Clone, Copy, Debug)]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Colour {
    pub fn from_gdk_rgba(
        RGBA {
            red,
            green,
            blue,
            alpha,
        }: RGBA,
    ) -> Self {
        Self {
            red: (red * 255.0).floor() as u8,
            green: (green * 255.0).floor() as u8,
            blue: (blue * 255.0).floor() as u8,
            alpha: (alpha * 255.0).floor() as u8,
        }
    }

    pub const BLACK: Self = Self {
        red: 0,
        green: 0,
        blue: 0,
        alpha: 255,
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rectangle {
    pub fn normalise(&mut self) {
        let Self { x, y, w, h } = self;
        if *w < 0.0 {
            *x += *w;
            *w = w.abs();
        }

        if *h < 0.0 {
            *y += *h;
            *h = h.abs();
        }
    }
}

/// A struct representing an ellipse
///
/// Properties:
/// * has a radius of w/2 (= a) in the x axis
/// * has a radius of h/2 (= b) in the y axis
/// * center is at (x + w/2, y + h/2) (= (x0, y0))
#[derive(Clone, Copy, Debug)]
pub struct Ellipse {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug)]
pub struct Text {
    pub string: String,
    pub font_description: FontDescription,
    pub colour: Colour,
}
