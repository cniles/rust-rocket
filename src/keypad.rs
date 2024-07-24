use std::rc::Rc;

use embedded_graphics::geometry::{Point, Size};

use crate::ui::{button::Button, ui::Ui};

const KEYPAD_LABELS: [&str; 12] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "CLR", "0", "ENT",
];

pub fn init_keypad<'a>(ui: &'a mut Ui, on_click: Box<dyn Fn(&str) -> ()>) {
    let origin = Point::new(100, 100);
    let size = Size::new(20, 20);
    let gap = Size::new(2, 2);

    let on_click = Rc::new(on_click);

    for i in 0..12 {
        let x = i % 3;
        let y = i / 3;

        let offset = origin + (size + gap).component_mul(Size::new(x, y));
        let label = KEYPAD_LABELS[i as usize].to_string();
        let label2 = KEYPAD_LABELS[i as usize].to_string();

        let on_click = on_click.clone();

        ui.add_element(Box::new(Button::new(
            offset,
            size,
            label2,
            Box::new(move || {
                on_click(&label);
            }),
        )));
    }
}
