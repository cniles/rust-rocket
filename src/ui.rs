use embedded_graphics::{
    geometry::{Dimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{Rgb565, RgbColor},
    primitives::{OffsetOutline, Primitive, PrimitiveStyleBuilder, Rectangle, StyledDrawable},
    text::Text,
    Drawable,
};
use ez_cyd_rs::CydDisplay;

#[derive(Copy, Clone, Debug)]
enum TouchStatus {
    Up,
    Down,
}

#[derive(Copy, Clone, Debug)]
struct TouchState {
    x: i32,
    y: i32,
    z: i32,
    status: TouchStatus,
}

pub struct Ui {
    width: u16,
    height: u16,

    elements: Vec<Box<dyn UiElement>>,

    touch_state: TouchState,
    touch_calibration: ((f64, f64), (f64, f64)),
}

#[derive(Copy, Clone, Debug)]
pub enum TouchEvent {
    Down(i32, i32),
    Up(i32, i32),
    Drag { from: (i32, i32), to: (i32, i32) },
    None,
}

const TOUCH_CALIBRATION: ((f64, f64), (f64, f64)) = (
    (-453.85041551246536, 267.09141274238226),
    (-476.5561372891216, 372.7632344386271),
);

const Z_THRESHOLD: f64 = 0.25;
const UP_THRESHOLD: i32 = 1;
const DOWN_THRESHOLD: i32 = -1;

#[derive(Copy, Clone, Debug)]
pub enum UiEvent {
    TouchEnter(TouchEvent),
    TouchLeave(TouchEvent),
    Tap(TouchEvent),
}

pub trait UiElement {
    fn handle_event(&mut self, event: UiEvent);
    fn dirty(&self) -> bool;
    fn bounding_box(&self) -> Rectangle;
    fn draw(&mut self, display: &mut CydDisplay);
}

impl Ui {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            touch_state: TouchState {
                x: 0,
                y: 0,
                z: 0,
                status: TouchStatus::Up,
            },
            elements: Vec::new(),
            touch_calibration: TOUCH_CALIBRATION,
        }
    }

    pub fn touch_calibration(&mut self, touch_calibration: ((f64, f64), (f64, f64))) {
        self.touch_calibration = touch_calibration;
    }

    pub fn add_element(&mut self, element: Box<dyn UiElement>) {
        self.elements.push(element);
    }

    pub fn clear(&mut self) {
        self.elements.clear();
    }

    pub fn draw(&mut self, display: &mut CydDisplay) {
        for e in self.elements.as_mut_slice() {
            if e.dirty() {
                e.draw(display);
            }
        }
    }

    fn process_touch(&mut self, touch: (f64, f64, f64)) -> TouchEvent {
        let (tx, ty, tz) = touch;

        // log::info!("raw touch: {:?}", touch);

        let ((ax, bx), (ay, by)) = self.touch_calibration;

        let x = (ay * ty + by) as i32;
        let y = (ax * tx + bx) as i32;

        log::info!("touch at {} {}", x, y);

        let status = if tz >= Z_THRESHOLD {
            TouchStatus::Down
        } else {
            TouchStatus::Up
        };

        // based on previous state
        let event = match self.touch_state.status {
            TouchStatus::Up => {
                if let TouchStatus::Down = status {
                    TouchEvent::Down(x, y)
                } else {
                    TouchEvent::None
                }
            }
            TouchStatus::Down => {
                if let TouchStatus::Up = status {
                    TouchEvent::Up(self.touch_state.x, self.touch_state.y)
                } else if x != self.touch_state.x || y != self.touch_state.y {
                    TouchEvent::Drag {
                        from: (self.touch_state.x, self.touch_state.y),
                        to: (x, y),
                    }
                } else {
                    TouchEvent::None
                }
            }
        };

        self.touch_state = TouchState { x, y, z: 0, status };

        event
    }

    pub fn handle_touch(&mut self, touch: (f64, f64, f64)) {
        let event = self.process_touch(touch);

        if let TouchEvent::None = event {
        } else {
            // log::info!("Received event {:?}", event);
        }

        for e in self.elements.as_mut_slice() {
            match event {
                TouchEvent::Down(x, y) => {
                    if e.bounding_box().contains((x, y).into()) {
                        e.handle_event(UiEvent::TouchEnter(event));
                    }
                }
                TouchEvent::Up(x, y) => {
                    if e.bounding_box().contains((x, y).into()) {
                        e.handle_event(UiEvent::Tap(event));
                    }
                }
                TouchEvent::Drag { from, to } => {
                    // did we enter or leave a button?
                    // for each component, check if x0,y0, and x1,y1 is in bounding box.
                    // if false false, do nothing.  if true false, notify left
                    // if false true, notify entered
                    // if true true, notify drag?
                    // if true false, notify left

                    let e0 = e.bounding_box().contains(from.into());
                    let e1 = e.bounding_box().contains(to.into());

                    if !e0 && e1 {
                        e.handle_event(UiEvent::TouchEnter(event));
                    }
                    if e0 && !e1 {
                        e.handle_event(UiEvent::TouchLeave(event));
                    }
                }
                TouchEvent::None => (),
            }
        }
    }
}

struct ColorTheme {
    text_color: Rgb565,
    fill: Rgb565,
    outline: Rgb565,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            text_color: Rgb565::BLACK,
            fill: Rgb565::BLACK,
            outline: Rgb565::BLACK,
        }
    }
}

pub struct Button<'a> {
    point: Point,
    size: Size,
    text: &'a str,
    color_theme: ColorTheme,
    hover_color_theme: ColorTheme,
    dirty: bool,
    hover: bool,
    on_click: Box<dyn Fn() -> ()>,
}

impl<'a> Button<'a> {
    pub fn new(point: Point, size: Size, text: &'a str, on_click: Box<dyn Fn() -> ()>) -> Self {
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

impl<'a> UiElement for Button<'a> {
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

        let mut text = Text::new(self.text, self.point, text_style);

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
