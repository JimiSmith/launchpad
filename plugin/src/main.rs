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
    }
    impl ZellijPlugin for Plugin {
        fn load(&mut self, _: std::collections::BTreeMap<String, String>) {
            // These events and CloseSelf need no permissions on Zellij 0.45.1.
            subscribe(&[EventType::Key, EventType::Mouse, EventType::PastedText]);
        }
        fn update(&mut self, event: Event) -> bool {
            let changed = self.state.handle(event);
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
