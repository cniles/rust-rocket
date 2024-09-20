use std::{cell::RefCell, rc::Rc};

use embedded_graphics::{
    geometry::{Dimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{Rgb565, RgbColor},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
    text::Text as GfxText,
    Drawable,
};

use super::ui::{ColorTheme, UiElement};

pub struct Text {
    text: Rc<RefCell<String>>,
    last_drawn: String,
    position: Point,
    color_theme: ColorTheme,
}

impl Text {
    pub fn text_ref(&self) -> Rc<RefCell<String>> {
        self.text.clone()
    }

    pub fn new(text: String, position: Point) -> Self {
        Self {
            text: Rc::new(RefCell::new(text)),
            position,
            color_theme: ColorTheme {
                text_color: Rgb565::GREEN,
                ..ColorTheme::default()
            },
            last_drawn: "".to_string(),
        }
    }
}

impl UiElement for Text {
    fn handle_event(&mut self, event: super::ui::UiEvent) {
        // don't care..
    }

    fn dirty(&self) -> bool {
        !self.text.borrow().eq(&self.last_drawn)
    }

    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(self.position, Size::new(5, 5))
    }

    fn draw(&mut self, display: &mut ez_cyd_rs::CydDisplay) {
        let style = PrimitiveStyle::with_fill(Rgb565::BLACK);
        let text_style = MonoTextStyle::new(&FONT_6X10, self.color_theme.text_color);
        let text: String = self.text.borrow().clone();

        // clear the previous area
        let gfx_text = GfxText::new(&self.last_drawn, self.position, text_style);
        gfx_text
            .bounding_box()
            .draw_styled(&style, display)
            .unwrap();

        // draw the new text
        let gfx_text = GfxText::new(&text, self.position, text_style);
        gfx_text.draw(display).unwrap();

        self.last_drawn = text;
    }
}
