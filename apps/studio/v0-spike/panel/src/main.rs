// Spike for #315: does a non-activating panel show without taking focus, and is the Dock icon hidden?
// Control: touch $SPIKE_DIR/show or $SPIKE_DIR/hide.
use std::{path::PathBuf, thread, time::Duration};
use tauri::{ActivationPolicy, Manager};
use tauri_nspanel::{tauri_panel, ManagerExt, WebviewWindowExt};

tauri_panel! {
    panel!(GuessPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

fn main() {
    let dir = PathBuf::from(std::env::var("SPIKE_DIR").expect("SPIKE_DIR"));
    tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .setup(move |app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            let panel = app.get_webview_window("panel").unwrap().to_panel::<GuessPanel>()?;
            panel
                .add_style_mask(tauri_nspanel::objc2_app_kit::NSWindowStyleMask::NonactivatingPanel)
                .expect("nonactivating");
            let handle = app.handle().clone();
            thread::spawn(move || loop {
                for cmd in ["show", "hide"] {
                    let f = dir.join(cmd);
                    if std::fs::remove_file(&f).is_ok() {
                        let h = handle.clone();
                        handle
                            .run_on_main_thread(move || {
                                let p = h.get_webview_panel("panel").unwrap();
                                if cmd == "show" { p.show() } else { p.hide() }
                                eprintln!("{cmd}: key={}", p.as_panel().isKeyWindow());
                            })
                            .unwrap();
                    }
                }
                thread::sleep(Duration::from_millis(200));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run");
}
