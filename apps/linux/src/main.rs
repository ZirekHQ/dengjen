//! The Dengjen app for Linux: paste or open text, hear it read aloud sentence by sentence,
//! and save it as a WAV file.

mod engine;
mod prosody;
mod speech;
mod window;

use adw::prelude::*;
use gtk::glib;

const APP_ID: &str = "io.github.zirekhq.Dengjen";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(window::present);
    app.set_accels_for_action("win.open", &["<Ctrl>o"]);
    app.set_accels_for_action("win.save", &["<Ctrl>s"]);
    app.set_accels_for_action("win.read-clipboard", &["<Ctrl><Shift>v"]);
    app.set_accels_for_action("win.play-pause", &["F5"]);
    app.set_accels_for_action("win.stop", &["Escape"]);
    app.set_accels_for_action("win.previous", &["<Alt>Left"]);
    app.set_accels_for_action("win.next", &["<Alt>Right"]);
    app.run()
}
