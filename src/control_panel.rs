use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};

use crate::ui::{button::Button, ui::Ui};

fn make_button<'a>(name: String, bp: &mut i32, on_click: Box<dyn Fn() -> ()>) -> Box<Button> {
    let result = Button::new((*bp, 215).into(), (25, 25).into(), name, on_click);
    *bp = *bp + 26;
    Box::new(result)
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

    ui.add_element({
        let cs = cs.clone();
        make_button(
            "TON".to_string(),
            &mut bp,
            Box::new(move || {
                cs.send("ton ".to_string()).unwrap();
            }),
        )
    });
    ui.add_element({
        let cs = cs.clone();
        make_button(
            "TOFF".to_string(),
            &mut bp,
            Box::new(move || {
                cs.send("toff ".to_string()).unwrap();
            }),
        )
    });
    ui.add_element({
        let cs = cs.clone();
        make_button(
            "TONE".to_string(),
            &mut bp,
            Box::new(move || {
                cs.send("tone ".to_string()).unwrap();
            }),
        )
    });
    ui.add_element({
        let cs = cs.clone();
        make_button(
            "RST".to_string(),
            &mut bp,
            Box::new(move || {
                cs.send("reset ".to_string()).unwrap();
            }),
        )
    });
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
