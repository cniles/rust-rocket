use embedded_graphics::{
    geometry::{Dimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{Rgb565, RgbColor},
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};
use ez_cyd_rs::CydDisplay;

use super::ui::{ColorTheme, UiElement, UiEvent};

pub struct Button {
    point: Point,
    size: Size,
    text: String,
    color_theme: ColorTheme,
    hover_color_theme: ColorTheme,
    dirty: bool,
    hover: bool,
    on_click: Box<dyn Fn() -> ()>,
}

impl Button {
    pub fn new(point: Point, size: Size, text: String, on_click: Box<dyn Fn() -> ()>) -> Self {
        Self {
            point,
            size,
            text,
            dirty: true,
            hover: false,
            color_theme: ColorTheme {
                text_color: Rgb565::GREEN,
                outline: Rgb565::GREEN,
                ..ColorTheme::default()
            },
            hover_color_theme: ColorTheme {
                fill: Rgb565::GREEN,
                outline: Rgb565::GREEN,
                ..ColorTheme::default()
            },
            on_click,
        }
    }
}

impl UiElement for Button {
    fn dirty(&self) -> bool {
        self.dirty
    }

    fn draw(&mut self, display: &mut CydDisplay) {
        let theme = if self.hover {
            &self.hover_color_theme
        } else {
            &self.color_theme
        };

        let style = PrimitiveStyleBuilder::new()
            .stroke_color(theme.outline)
            .fill_color(theme.fill)
            .stroke_width(1)
            .build();

        Rectangle::new(self.point, self.size)
            .into_styled(style)
            .draw(display)
            .unwrap();

        let text_style = MonoTextStyle::new(&FONT_6X10, theme.text_color);

        let mut text = Text::new(&self.text, self.point, text_style);

        let text_size = text.bounding_box().size;

        text.position = (
            self.size.width as i32 / 2 - text_size.width as i32 / 2 + self.point.x,
            self.size.height as i32 / 2 + text_size.height as i32 / 2 + self.point.y,
        )
            .into();

        text.draw(display).unwrap();

        self.dirty = false;
    }

    fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: self.point,
            size: self.size,
        }
    }

    fn handle_event(&mut self, event: UiEvent) {
        // log::info!("Ui Event: {:?}", event);
        self.dirty = true;
        match event {
            UiEvent::TouchEnter(_) => {
                self.hover = true;
            }
            UiEvent::TouchLeave(_) => {
                self.hover = false;
            }
            UiEvent::Tap(_) => {
                self.hover = false;
                (*self.on_click)();
            }
        }
    }
}
