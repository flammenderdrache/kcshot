use diesel::SqliteConnection;
use gtk4::{
    gdk::{self, prelude::*},
    gdk_pixbuf::Pixbuf,
    gio,
};

use crate::historymodel::HistoryModel;

/// Trait for the post capture actions.
pub trait PostCaptureAction {
    /// The name of the post capture action.
    fn name(&self) -> String;

    /// Short description of the post capture action.
    fn description(&self) -> String;

    /// Gets called when executing the post capture action.
    fn handle(&self, history_model: &HistoryModel, conn: &SqliteConnection, pixbuf: &mut Pixbuf);
}

/// This struct represents the action of saving the pixbuf to disk.
pub struct SaveToDisk;

impl PostCaptureAction for SaveToDisk {
    fn name(&self) -> String {
        "Save to disk".to_owned()
    }

    fn description(&self) -> String {
        "Saves the screenshot to the Harddrive".to_owned()
    }

    fn handle(&self, history_model: &HistoryModel, conn: &SqliteConnection, pixbuf: &mut Pixbuf) {
        let now = chrono::Local::now();

        let settings = gio::Settings::new("kc.kcshot");
        let path = settings.string("saved-screenshots-path");
        let path = if path.ends_with('/') {
            format!("{}screenshot_{}.png", path, now.to_rfc3339())
        } else {
            format!("{}/screenshot_{}.png", path, now.to_rfc3339())
        };

        let res = pixbuf.savev(&path, "png", &[]);

        match res {
            Ok(_) => {}
            Err(why) => tracing::error!("Failed to save screenshot to file: {}", why),
        }

        history_model.add_item_to_history(conn, Some(path), now.to_rfc3339(), None);
    }
}

/// This struct represents the action of copying the picture to the users clipboard.
pub struct CopyToClipboard;

impl PostCaptureAction for CopyToClipboard {
    fn name(&self) -> String {
        "Copy to clipboard".to_owned()
    }

    fn description(&self) -> String {
        "Copies the picture to the clipboard".to_owned()
    }

    fn handle(&self, _history_mod: &HistoryModel, _conn: &SqliteConnection, pixbuf: &mut Pixbuf) {
        let display = match gdk::Display::default() {
            Some(display) => display,
            None => {
                tracing::error!("Failed to fetch gdk::Display, bailing...");
                return;
            }
        };
        let clipboard = display.clipboard();

        clipboard.set_texture(&gdk::Texture::for_pixbuf(&pixbuf));
    }
}

/// Vector of all available post capture actions.
pub fn get_postcapture_actions() -> Vec<&'static dyn PostCaptureAction> {
    vec![&SaveToDisk, &CopyToClipboard]
}
