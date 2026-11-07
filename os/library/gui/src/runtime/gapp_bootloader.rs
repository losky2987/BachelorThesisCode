use concurrent::{process, thread};
use drawer::{rect_data::RectData, vertex::Vertex};
use terminal::println;

use crate::api::current_component_frame;

use super::process::{defer_surface_updates, register_process_surface, run_loop_iter_fns, run_process_event_iter};

/// Bootstrap one standalone graphical application process.
pub fn run(entry: fn()) {
    let app_id = process::current().expect("Graphical application process not found").id();
    let mut args = runtime::env::args().skip(1);
    let Some(workspace_index) = args.next().and_then(|value| value.parse::<usize>().ok()) else {
        print_gui_environment_warning();
        return;
    };
    let Some(window_id) = args.next().and_then(|value| value.parse::<usize>().ok()) else {
        print_gui_environment_warning();
        return;
    };
    let Some(screen_width) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
        print_gui_environment_warning();
        return;
    };
    let Some(screen_height) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
        print_gui_environment_warning();
        return;
    };
    let Some(input_queue_generation) = args.next().and_then(|value| value.parse::<u64>().ok()) else {
        print_gui_environment_warning();
        return;
    };

    register_process_surface(
        app_id,
        workspace_index,
        window_id,
        input_queue_generation,
        RectData {
            top_left: Vertex::zero(),
            width: 0,
            height: 0,
        },
        (screen_width, screen_height),
    )
    .expect("Failed to initialize GUI process surface");

    // Window geometry is owned by the Window Manager. Wait for libgui to read
    // its initial RectData from the shared input queue before constructing and
    // rendering the component tree.
    loop {
        if !run_process_event_iter() {
            return;
        }
        let ready = current_component_frame()
            .map(|frame| {
                let rect = frame.component_frame.read().rect();
                rect.width > 0 && rect.height > 0
            })
            .unwrap_or(false);
        if ready {
            break;
        }
        thread::sleep(1);
    }

    // Application constructors commonly add many components. Publishing a
    // complete shared-memory frame after every add caused large startup stalls.
    defer_surface_updates(true);
    entry();
    defer_surface_updates(false);
    if let Ok(surface) = current_component_frame() {
        let _ = surface.update_frame();
    }

    while run_process_event_iter() {
        run_loop_iter_fns();
        thread::sleep(1);
    }
}

fn print_gui_environment_warning() {
    println!("This program is graphical program, only running under GUI ENV, please press F1 to enter GUI ENV.");
}
