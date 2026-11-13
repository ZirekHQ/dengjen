//! The main window: a text area, playback controls, the voice, speed and pitch.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;

use adw::prelude::*;
use dengjen_app::import::{self, Format};
use dengjen_app::{sentence_at, sentences, Sentence};
use gtk::{gio, glib};

use crate::engine::{self, Engine, Voice};
use crate::prosody;
use crate::speech::{self, Command, Event};

const HIGHLIGHT: &str = "reading";
const PLAY_ICON: &str = "media-playback-start-symbolic";
const PAUSE_ICON: &str = "media-playback-pause-symbolic";

/// The text being read, split when reading started.
struct Reading {
    sentences: Vec<Sentence>,
    texts: Vec<String>,
    current: Option<usize>,
    paused: bool,
}

struct Ui {
    window: adw::ApplicationWindow,
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    voices: gtk::DropDown,
    voice_keys: Vec<String>,
    speed: gtk::Scale,
    pitch: gtk::Scale,
    play: gtk::Button,
    toasts: adw::ToastOverlay,
    commands: mpsc::Sender<Command>,
    reading: RefCell<Option<Reading>>,
    /// The text the highlight offsets refer to.
    text: RefCell<String>,
}

/// Shows the window, building it on first activation.
pub fn present(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    build(app).window.present();
}

/// Characters before byte `byte` of `text`, the offset GTK text iterators count in.
fn char_offset(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |head| head.chars().count())
}

/// The sentence to start from with the caret `caret` characters into `text`: the one holding
/// the caret, or the first when the caret is past the last sentence.
fn start_index(text: &str, sentences: &[Sentence], caret: usize) -> usize {
    let utf16 = text.chars().take(caret).map(char::len_utf16).sum();
    sentence_at(sentences, utf16).unwrap_or(0)
}

fn gtk_offset(offset: usize) -> i32 {
    i32::try_from(offset).unwrap_or(i32::MAX)
}

fn icon_button(icon: &str, label: &str, action: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(label));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button.set_action_name(Some(action));
    button
}

fn slider(label: &str) -> (gtk::Label, gtk::Scale) {
    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        prosody::SLIDER_MIN,
        prosody::SLIDER_MAX,
        10.0,
    );
    scale.set_value(0.0);
    scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    scale.set_hexpand(true);
    let text = gtk::Label::with_mnemonic(label);
    text.set_mnemonic_widget(Some(&scale));
    scale.update_relation(&[gtk::accessible::Relation::LabelledBy(&[text.upcast_ref()])]);
    (text, scale)
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let voices_dir = engine::voices_dir().unwrap_or_default();
    let voice_keys = engine::installed_voices(&voices_dir);

    let buffer = gtk::TextBuffer::new(None);
    buffer.create_tag(
        Some(HIGHLIGHT),
        &[("background", &"#f6d32d"), ("foreground", &"#000000")],
    );
    let view = gtk::TextView::with_buffer(&buffer);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_left_margin(12);
    view.set_right_margin(12);
    view.set_top_margin(12);
    view.set_bottom_margin(12);
    view.update_property(&[gtk::accessible::Property::Label("Text to read")]);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .build();

    let banner = adw::Banner::new(&format!(
        "No voice is installed. Install one with: dengjen voices install {}",
        engine::SUGGESTED_VOICE
    ));
    banner.set_revealed(voice_keys.is_empty());

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&banner);
    content.append(&scroller);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&content));

    let header = adw::HeaderBar::new();
    header.pack_start(&icon_button(
        "document-open-symbolic",
        "Open a file",
        "win.open",
    ));
    header.pack_start(&icon_button(
        "edit-paste-symbolic",
        "Read the clipboard",
        "win.read-clipboard",
    ));
    header.pack_end(&icon_button(
        "document-save-symbolic",
        "Save as audio",
        "win.save",
    ));

    let play = icon_button(PLAY_ICON, "Play", "win.play-pause");
    let transport = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    transport.set_halign(gtk::Align::Center);
    transport.append(&icon_button(
        "media-skip-backward-symbolic",
        "Previous sentence",
        "win.previous",
    ));
    transport.append(&play);
    transport.append(&icon_button(
        "media-playback-stop-symbolic",
        "Stop",
        "win.stop",
    ));
    transport.append(&icon_button(
        "media-skip-forward-symbolic",
        "Next sentence",
        "win.next",
    ));

    let names: Vec<&str> = voice_keys.iter().map(String::as_str).collect();
    let voices = gtk::DropDown::from_strings(&names);
    voices.update_property(&[gtk::accessible::Property::Label("Voice")]);
    let (speed_label, speed) = slider("_Speed");
    let (pitch_label, pitch) = slider("_Pitch");
    let settings = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    settings.append(&voices);
    settings.append(&speed_label);
    settings.append(&speed);
    settings.append(&pitch_label);
    settings.append(&pitch);

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for margin in [
        gtk::Widget::set_margin_top,
        gtk::Widget::set_margin_bottom,
        gtk::Widget::set_margin_start,
        gtk::Widget::set_margin_end,
    ] {
        margin(controls.upcast_ref(), 6);
    }
    controls.append(&transport);
    controls.append(&settings);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_bottom_bar(&controls);
    toolbar.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Dengjen")
        .default_width(760)
        .default_height(540)
        .content(&toolbar)
        .build();

    let (commands, command_receiver) = mpsc::channel();
    let (events, event_receiver) = async_channel::unbounded();
    std::thread::spawn(move || {
        let mut engine = Engine::new(voices_dir);
        speech::run(&mut engine, &command_receiver, &events);
    });

    let ui = Rc::new(Ui {
        window,
        view,
        buffer,
        voices,
        voice_keys,
        speed,
        pitch,
        play,
        toasts,
        commands,
        reading: RefCell::new(None),
        text: RefCell::new(String::new()),
    });
    add_actions(&ui);

    let weak = Rc::downgrade(&ui);
    glib::spawn_future_local(async move {
        while let Ok(event) = event_receiver.recv().await {
            match weak.upgrade() {
                Some(ui) => ui.on_event(event),
                None => break,
            }
        }
    });
    ui
}

type Handler = fn(&Rc<Ui>);

fn add_actions(ui: &Rc<Ui>) {
    let actions: [(&str, Handler); 7] = [
        ("open", Ui::open),
        ("save", Ui::save),
        ("read-clipboard", Ui::read_clipboard),
        ("play-pause", Ui::play_pause),
        ("stop", |ui| ui.stop()),
        ("previous", |ui| ui.skip(-1)),
        ("next", |ui| ui.skip(1)),
    ];
    for (name, activate) in actions {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(ui);
        action.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                activate(&ui);
            }
        });
        ui.window.add_action(&action);
    }
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            self.toast("The speech engine stopped. Restart the app.");
        }
    }

    fn current_text(&self) -> String {
        let (start, end) = self.buffer.bounds();
        self.buffer.text(&start, &end, false).into()
    }

    fn voice(&self) -> Option<Voice> {
        let key = usize::try_from(self.voices.selected())
            .ok()
            .and_then(|index| self.voice_keys.get(index));
        let Some(key) = key else {
            self.toast("Install a voice first.");
            return None;
        };
        Some(Voice {
            key: key.clone(),
            rate: prosody::engine_rate(self.speed.value()),
            pitch: prosody::engine_pitch(self.pitch.value()),
        })
    }

    fn set_playing(&self, playing: bool) {
        let (icon, label) = if playing {
            (PAUSE_ICON, "Pause")
        } else {
            (PLAY_ICON, "Play")
        };
        self.play.set_icon_name(icon);
        self.play.set_tooltip_text(Some(label));
        self.play
            .update_property(&[gtk::accessible::Property::Label(label)]);
    }

    fn highlight(&self, sentence: Option<&Sentence>) {
        let (start, end) = self.buffer.bounds();
        self.buffer.remove_tag_by_name(HIGHLIGHT, &start, &end);
        let Some(sentence) = sentence else {
            return;
        };
        let text = self.text.borrow();
        let mut from = self
            .buffer
            .iter_at_offset(gtk_offset(char_offset(&text, sentence.bytes.start)));
        let to = self
            .buffer
            .iter_at_offset(gtk_offset(char_offset(&text, sentence.bytes.end)));
        self.buffer.apply_tag_by_name(HIGHLIGHT, &from, &to);
        self.buffer.place_cursor(&from);
        self.view.scroll_to_iter(&mut from, 0.1, false, 0.0, 0.0);
    }

    /// Reads from sentence `index` of the current reading, or starts a new reading from the
    /// caret.
    fn play_from(&self, index: Option<usize>) {
        let Some(voice) = self.voice() else {
            return;
        };
        let mut reading = self.reading.borrow_mut();
        let reading = match (reading.as_mut(), index) {
            (Some(existing), Some(_)) => existing,
            _ => {
                let text = self.current_text();
                let found = sentences(&text);
                if found.is_empty() {
                    self.toast("There is nothing to read.");
                    return;
                }
                let texts = found
                    .iter()
                    .map(|sentence| sentence.text(&text).unwrap_or_default().to_owned())
                    .collect();
                *self.text.borrow_mut() = text;
                reading.insert(Reading {
                    sentences: found,
                    texts,
                    current: None,
                    paused: false,
                })
            }
        };
        let start = index.unwrap_or_else(|| {
            let caret = usize::try_from(self.buffer.cursor_position()).unwrap_or(0);
            start_index(&self.text.borrow(), &reading.sentences, caret)
        });
        reading.paused = false;
        self.send(Command::Play {
            sentences: reading.texts.clone(),
            start,
            voice,
        });
        self.view.set_editable(false);
        self.set_playing(true);
    }

    fn play_pause(self: &Rc<Self>) {
        let mut reading = self.reading.borrow_mut();
        match reading.as_mut() {
            Some(reading) => {
                reading.paused = !reading.paused;
                let paused = reading.paused;
                self.send(if paused {
                    Command::Pause
                } else {
                    Command::Resume
                });
                self.set_playing(!paused);
            }
            None => {
                drop(reading);
                self.play_from(None);
            }
        }
    }

    fn skip(&self, delta: isize) {
        let target = self.reading.borrow().as_ref().map(|reading| {
            let current = reading.current.unwrap_or(0);
            current
                .saturating_add_signed(delta)
                .min(reading.sentences.len().saturating_sub(1))
        });
        if let Some(target) = target {
            self.play_from(Some(target));
        }
    }

    fn finish(&self) {
        *self.reading.borrow_mut() = None;
        self.highlight(None);
        self.view.set_editable(true);
        self.set_playing(false);
    }

    fn stop(&self) {
        if self.reading.borrow().is_some() {
            self.send(Command::Stop);
            self.finish();
        }
    }

    fn on_event(&self, event: Event) {
        match event {
            Event::Started(index) => {
                let sentence = self.reading.borrow_mut().as_mut().and_then(|reading| {
                    reading.current = Some(index);
                    reading.sentences.get(index).cloned()
                });
                self.highlight(sentence.as_ref());
            }
            Event::Finished => self.finish(),
            Event::Saved(path) => self.toast(&format!("Saved {}", path.display())),
            Event::Failed(error) => {
                self.finish();
                self.toast(&error);
            }
        }
    }

    fn set_text(&self, text: &str) {
        self.stop();
        self.buffer.set_text(text);
        self.buffer.place_cursor(&self.buffer.start_iter());
    }

    fn load(&self, path: &PathBuf) {
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(Format::from_extension)
            .unwrap_or(Format::PlainText);
        let text = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                import::text_from_bytes(&bytes, format).map_err(|error| error.to_string())
            });
        match text {
            Ok(text) => self.set_text(&text),
            Err(error) => self.toast(&format!("Could not open {}: {error}", path.display())),
        }
    }

    fn open(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Text, Markdown and HTML"));
        for pattern in [
            "*.txt",
            "*.text",
            "*.md",
            "*.markdown",
            "*.htm",
            "*.html",
            "*.xhtml",
        ] {
            filter.add_pattern(pattern);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Open a file to read")
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let weak = Rc::downgrade(self);
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let path = result.ok().and_then(|file| file.path());
            if let (Some(ui), Some(path)) = (weak.upgrade(), path) {
                ui.load(&path);
            }
        });
    }

    fn save(self: &Rc<Self>) {
        let text = self.current_text();
        if sentences(&text).is_empty() {
            self.toast("There is nothing to save.");
            return;
        }
        let Some(voice) = self.voice() else {
            return;
        };
        let dialog = gtk::FileDialog::builder()
            .title("Save as audio")
            .initial_name("dengjen.wav")
            .build();
        let weak = Rc::downgrade(self);
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let path = result.ok().and_then(|file| file.path());
            if let (Some(ui), Some(path)) = (weak.upgrade(), path) {
                ui.toast("Saving…");
                ui.send(Command::Save { text, voice, path });
            }
        });
    }

    fn read_clipboard(self: &Rc<Self>) {
        let clipboard = self.view.clipboard();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let text = clipboard.read_text_future().await;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match text {
                Ok(Some(text)) if !text.trim().is_empty() => {
                    ui.set_text(&text);
                    ui.play_from(None);
                }
                _ => ui.toast("The clipboard has no text."),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_offsets_count_characters_not_bytes() {
        let text = "Çawa yî? Baş im.";
        assert_eq!(char_offset(text, 0), 0);
        assert_eq!(char_offset(text, "Çawa".len()), 4);
        assert_eq!(char_offset(text, text.len()), 16);
        assert_eq!(char_offset(text, 1), 0);
    }

    #[test]
    fn reading_starts_at_the_sentence_under_the_caret() {
        let text = "Rojbaş. Çawa yî? Ez baş im.";
        let found = sentences(text);
        assert_eq!(start_index(text, &found, 0), 0);
        assert_eq!(start_index(text, &found, 10), 1);
        assert_eq!(start_index(text, &found, 20), 2);
        assert_eq!(start_index(text, &found, 100), 0);
    }
}
