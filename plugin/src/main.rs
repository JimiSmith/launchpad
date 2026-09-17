// SDK macro uses pre-2024 #[no_mangle], hence this thin package is edition 2021.
#[cfg(target_family = "wasm")]
mod wasm {
    use launchpad_plugin::State;
    use zellij_tile::prelude::*;
    register_plugin!(Plugin);
    pub fn initialize() {
        main();
    }

    #[derive(Default)]
    struct Plugin {
        state: State,
        timer_pending: bool,
    }
    impl ZellijPlugin for Plugin {
        fn load(&mut self, configuration: std::collections::BTreeMap<String, String>) {
            subscribe(&[
                EventType::Key,
                EventType::Mouse,
                EventType::PastedText,
                EventType::PermissionRequestResult,
                EventType::HostFolderChanged,
                EventType::FailedToChangeHostFolder,
                EventType::Timer,
            ]);
            if configuration.get("demo").is_some_and(|v| v == "true") {
                self.state.app = zellij_launchpad_prototype::app::App::demo();
            } else {
                self.state.app.search_status = "Waiting for HOME permissions…".into();
                request_permission(&[
                    PermissionType::ReadSessionEnvironmentVariables,
                    PermissionType::FullHdAccess,
                ]);
            }
        }
        fn update(&mut self, event: Event) -> bool {
            if matches!(
                event,
                Event::PermissionRequestResult(PermissionStatus::Granted)
            ) && !self.state.app.is_demo()
            {
                // WASI HOME is not the host HOME. Consume only HOME from the SDK;
                // never log or retain the rest of the session environment.
                let home = get_session_environment_variables().remove("HOME");
                if self.state.prepare_home(home.clone()) {
                    change_host_folder(home.unwrap().into());
                }
                return true;
            }
            if matches!(event, Event::Timer(_)) {
                self.timer_pending = false;
            }
            let changed = self.state.handle(event);
            if self.state.app.is_indexing() && !self.timer_pending {
                set_timeout(0.01);
                self.timer_pending = true;
            }
            if self.state.app.quit {
                close_self();
                return false;
            }
            changed
        }
        fn render(&mut self, rows: usize, cols: usize) {
            // No newline: exact-fit bottom-right cells must not scroll.
            print!("{}", self.state.render_frame(rows, cols));
        }
    }
}
// register_plugin!'s main initializes the SDK panic hook.
#[cfg(target_family = "wasm")]
fn main() {
    wasm::initialize();
}
#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("Build this plugin for wasm32-wasip1; use the root package for the native TUI.");
}
