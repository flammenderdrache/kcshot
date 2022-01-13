use std::collections::HashMap;

use diesel::SqliteConnection;
use gtk4::{
    gdk::{self, prelude::*},
    gdk_pixbuf::Pixbuf,
    gio,
};

use crate::historymodel::HistoryModel;

/// Trait for the post capture actions.
pub trait PostCaptureAction {
    /// Returns the ID of the action, this is used for the settings.
    fn id(&self) -> String;

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
    fn id(&self) -> String {
        "SaveToDisk".to_owned()
    }

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
    fn id(&self) -> String {
        "CopyToClipboard".to_owned()
    }

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

/// Executes the post capture actions in the order they are defined in the settings.
pub fn do_postcapture_actions(history_model: &HistoryModel, conn: &SqliteConnection, pixbuf: &mut Pixbuf) {
    for action in get_actions_from_settings() {
        action.handle(history_model, conn, pixbuf)
    }
}

fn get_actions_from_settings() -> Vec<&'static dyn PostCaptureAction> {
    let settings = gio::Settings::new("kc.kcshot");
    let actions: Vec<String> = settings.strv("post-capture-actions")
        .iter()
        .map(|gstr| gstr.to_string())
        .collect();

    let action_lookup: HashMap<String, &dyn PostCaptureAction> = get_postcapture_actions()
        .iter()
        .map(|action| (action.id(), *action))
        .collect();


    let mut actions_todo = Vec::new();
    for postcapture_action in actions {
        if let Some(action) = action_lookup.get(postcapture_action.as_str()) {
            actions_todo.push(*action)
        } else {
            tracing::warn!(
                "Found post capture action `{}` in the settings, but not in list of available post capture actions!",
                postcapture_action
            )
        }
    }

    actions_todo
}

/// Vector of all available post capture actions.
fn get_postcapture_actions() -> Vec<&'static dyn PostCaptureAction> {
    vec![&SaveToDisk, &CopyToClipboard]
}
