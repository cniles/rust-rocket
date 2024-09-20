use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
};

use crate::ui::{button::Button, ui::Ui};

fn make_button(name: String, bp: &mut i32, on_click: Box<dyn Fn() -> ()>) -> Box<Button> {
    let result = Button::new((*bp, 215).into(), (25, 25).into(), name, on_click);
    *bp = *bp + 26;
    Box::new(result)
}

fn make_command_button<'a>(
    label: &'static str,
    cmd: &'static str,
    bp: &mut i32,
    cs: Sender<String>,
) -> Box<Button> {
    make_button(
        label.to_string().to_uppercase(),
        bp,
        Box::new(move || {
            cs.send(cmd.to_string()).unwrap();
        }),
    )
}

pub fn init_control_panel<'a>(
    command_sender: Sender<String>,
    ui: &'a mut Ui,
) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
    let mut bp = 1;

    let cs = command_sender.clone();
    let clear_flag = Arc::new(AtomicBool::new(false));
    let psl_flag = Arc::new(AtomicBool::new(false));
    let cf = clear_flag.clone();
    let pf = psl_flag.clone();

    ui.add_element(make_command_button("ton", "ton", &mut bp, cs.clone()));
    ui.add_element(make_command_button("toff", "toff", &mut bp, cs.clone()));
    ui.add_element(make_command_button("tone", "tone", &mut bp, cs.clone()));
    ui.add_element(make_command_button("rst", "reset", &mut bp, cs.clone()));

    ui.add_element(make_button(
        "CLR".to_string(),
        &mut bp,
        Box::new(move || {
            cf.store(true, Ordering::Relaxed);
        }),
    ));
    ui.add_element(make_button(
        "PSL".to_string(),
        &mut bp,
        Box::new(move || {
            pf.store(true, Ordering::Relaxed);
        }),
    ));

    (clear_flag, psl_flag)
}
