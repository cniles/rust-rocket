use std::rc::Rc;

use embedded_graphics::geometry::{Point, Size};

use crate::ui::{button::Button, text::Text, ui::Ui};

const KEYPAD_LABELS: [&str; 15] = [
    "", "", "x", "1", "2", "3", "4", "5", "6", "7", "8", "9", "CLR", "0", "ENT",
];

pub fn init_keypad<'a>(
    ui: &'a mut Ui,
    on_enter: Box<dyn Fn(&str) -> ()>,
    on_exit: Box<dyn Fn() -> ()>,
) {
    let origin = Point::new(50, 75);
    let size = Size::new(20, 20);
    let gap = Size::new(2, 2);
    let text = Text::new("".to_string(), origin - Point::new(0, 5));
    let keypad_value = text.text_ref();

    ui.add_element(Box::new(text));

    let click_handler = Rc::new(Box::new(move |label: &str| {
        if label == "CLR" {
            keypad_value.borrow_mut().clear();
        } else if label == "ENT" {
            let value: String = keypad_value.borrow_mut().clone();
            on_enter(&value);
        } else if label == "x" {
            on_exit();
        } else if label == "" {
            // doot
        } else {
            keypad_value.borrow_mut().push_str(label);
        }
    }));

    for i in 0..KEYPAD_LABELS.len() {
        let x = i % 3;
        let y = i / 3;

        let offset = origin + (size + gap).component_mul(Size::new(x as u32, y as u32));
        let label = KEYPAD_LABELS[i as usize].to_string();
        let label2 = KEYPAD_LABELS[i as usize].to_string();

        let click_handler = click_handler.clone();

        ui.add_element(Box::new(Button::new(
            offset,
            size,
            label2,
            Box::new(move || {
                (click_handler)(&label);
            }),
        )));
    }
}
