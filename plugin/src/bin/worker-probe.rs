//! Throwaway gate: remount then reload must remount workers too.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zellij_tile::prelude::*;
register_plugin!(Probe);
register_worker!(Worker, mapping_worker, MAPPING);
#[derive(Default, Serialize, Deserialize)]
struct Worker;
impl ZellijWorker<'_> for Worker {
    fn on_message(&mut self, _: String, _: String) {
        let cwd = get_plugin_ids().initial_cwd;
        let witness = std::path::Path::new("/host/worker-home-witness").is_dir();
        post_message_to_plugin(PluginMessage::new_to_plugin(
            "mapping",
            &format!("{}|{}", cwd.display(), witness),
        ));
    }
}
#[derive(Default)]
struct Probe {
    home: String,
    initial: String,
    status: String,
}
impl ZellijPlugin for Probe {
    fn load(&mut self, _: BTreeMap<String, String>) {
        self.initial = get_plugin_ids().initial_cwd.display().to_string();
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::HostFolderChanged,
            EventType::CustomMessage,
        ]);
        self.status = "Waiting for permissions".into();
        request_permission(&[
            PermissionType::ReadSessionEnvironmentVariables,
            PermissionType::FullHdAccess,
            PermissionType::ChangeApplicationState,
        ]);
    }
    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.home = get_session_environment_variables().remove("HOME").unwrap();
                if self.initial == self.home {
                    post_message_to(PluginMessage::new_to_worker("mapping", "hello", ""));
                } else if std::path::Path::new("/data/reload-attempted").exists() {
                    self.status = "BLOCKED reload did not retain HOME".into();
                } else {
                    self.status = "Waiting for HOME remount".into();
                    change_host_folder(self.home.clone().into());
                }
            }
            Event::HostFolderChanged(path) if path.to_str() == Some(self.home.as_str()) => {
                std::fs::write("/data/reload-attempted", b"1").unwrap();
                self.status = "Reloading once".into();
                reload_plugin_with_id(get_plugin_ids().plugin_id);
            }
            Event::CustomMessage(name, payload) if name == "mapping" => {
                let expected = format!("{}|true", self.home);
                self.status = if payload == expected {
                    "GATE PASS worker HOME mapping".into()
                } else {
                    format!("GATE FAIL {payload}")
                };
                eprintln!(
                    "MAPPING_PROBE initial={} home={} worker={payload}",
                    self.initial, self.home
                );
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                self.status = "GATE denied safely".into()
            }
            _ => {}
        };
        true
    }
    fn render(&mut self, _: usize, _: usize) {
        println!("{}", self.status);
    }
}
