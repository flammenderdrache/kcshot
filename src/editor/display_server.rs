use std::{
    ffi::CString,
    io,
    os::{raw::c_char, unix::prelude::OsStrExt},
};

use cairo::{
    self,
    ffi::{
        cairo_status_t, cairo_surface_status, cairo_surface_t, cairo_xcb_surface_create,
        STATUS_SUCCESS as CAIRO_STATUS_SUCCESS,
    },
    Error as CairoError, ImageSurface,
};
use gtk4::prelude::{FileExt, IOStreamExt, InputStreamExtManual};
use tracing::error;
use xcb::{
    shape,
    x::{self, MapState, Window as XWindow, ATOM_CARDINAL, ATOM_NONE, ATOM_WINDOW},
    Xid, XidNew,
};

use super::data::Rectangle;

extern "C" {
    /// cairo-rs doesn't expose it in its ffi module, so I have to write its declaration myself
    /// Here's the cairo docs for it: https://cairographics.org/manual/cairo-PNG-Support.html#cairo-surface-write-to-png
    fn cairo_surface_write_to_png(
        surface: *mut cairo_surface_t,
        filename: *const c_char,
    ) -> cairo_status_t;
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Encountered an error from cairo: {0}")]
    Cairo(#[from] CairoError),
    #[error("Encountered an I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Failed to estabilish a connection to the X server: {0:?}")]
    XcbConnection(#[from] xcb::ConnError),
    #[error("Encountered an X protocol error: {0:?}")]
    XcbProtocol(xcb::ProtocolError),
    #[error("Got an error trying to make a temporary file: {0}")]
    TempFile(#[from] gtk4::glib::Error),
    #[error("Failed to take screenshot. (No root screens? No cursor?)")]
    FailedToTakeScreenshot,
    #[error("Failed to get screen resolution. (No root screens? No root windows on the screens that exist?")]
    FailedToGetScreenResolution,
    #[error("WM does not support EWMH")]
    WmDoesNotSupportEwmh,
    #[error("Failed to get windows")]
    FailedToGetWindows,
}

impl From<cairo::IoError> for Error {
    fn from(cerr: cairo::IoError) -> Self {
        match cerr {
            cairo::IoError::Cairo(cairo) => Self::Cairo(cairo),
            cairo::IoError::Io(io) => Self::Io(io),
        }
    }
}

impl From<xcb::Error> for Error {
    fn from(xerror: xcb::Error) -> Self {
        match xerror {
            xcb::Error::Connection(err) => Self::XcbConnection(err),
            xcb::Error::Protocol(err) => Self::XcbProtocol(err),
        }
    }
}

#[derive(Debug)]
pub struct Window {
    /// This fields contains the rect of the window that also encompasses window decorations
    pub outer_rect: Rectangle,
    /// This fields contains the rect of the window that **only** encompasses the content
    pub content_rect: Rectangle,
}

// Some things in this file are inspired by the code written here https://giters.com/psychon/x11rb/issues/328
// archive.org link: https://web.archive.org/web/20220109220701/https://giters.com/psychon/x11rb/issues/328 [1]

pub fn take_screenshot() -> Result<ImageSurface, Error> {
    let (connection, _) = xcb::Connection::connect(None)?;
    let setup = connection.get_setup();

    for root_screen in setup.roots() {
        let window = root_screen.root();
        let pointer_cookie = connection.send_request(&x::QueryPointer { window });
        let geometry_cookie = connection.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(window),
        });

        let pointer_reply = connection.wait_for_reply(pointer_cookie)?;
        if pointer_reply.same_screen() {
            let geometry_reply = connection.wait_for_reply(geometry_cookie)?;
            let mut visualtype = match find_xcb_visualtype(&connection, root_screen.root_visual()) {
                Some(visualtype) => visualtype,
                None => continue,
            };
            let raw_connection = connection.get_raw_conn();
            let width = geometry_reply.width() as i32;
            let height = geometry_reply.height() as i32;

            // SAFETY: cairo doesn't touch the pointers we give it, so
            //      * the connection should be fine to pass through,
            //      * the visualtype is (hopefully) ABI compatible with C
            // Also see [1]
            let screenshot = unsafe {
                cairo_xcb_surface_create(
                    raw_connection as _,
                    window.resource_id(),
                    &mut visualtype as *mut _ as _,
                    width,
                    height,
                )
            };

            let surface_status = unsafe { cairo_surface_status(screenshot) };
            if surface_status != CAIRO_STATUS_SUCCESS {
                return Err(CairoError::from(surface_status).into());
            }

            let (file, stream) = gtk4::gio::File::new_tmp("screenshot.XXXXXX.png")?;
            let path = file.path().unwrap();
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();

            // SAFETY: * screenshot is a valid surface (see above the `cairo_surface_status` call)
            //         *  path is a valid nul terminated c-string (we should've bailed out above otherwise)
            match unsafe { cairo_surface_write_to_png(screenshot, path.as_ptr()) } {
                0 => {}
                err => return Err(CairoError::from(err).into()),
            }

            // Why do we this instead of just returning an XcbSurface?
            // When I first started experimenting with writing a screenshot-utility, I did it in C++
            // using xlib and Cairo::XlibSurface. That had some behaviour I disliked: when switching
            // tags, the surface displayed the contents of the new tag, instead of the old one. This
            // happened when I tested cairo-rs's XcbSurface, I assume it'll be the same, so we end
            // up writing the screenshot to a .png and then reading it again

            return Ok(ImageSurface::create_from_png(
                &mut stream.input_stream().into_read(),
            )?);
        }
    }

    Err(Error::FailedToTakeScreenshot)
}

/// Obtains a list of all windows from the display server, the list is in stacking order.
pub fn get_windows() -> Result<Vec<Window>, Error> {
    let (connection, _) = xcb::Connection::connect(None)?;
    let setup = connection.get_setup();

    // Requires an WM that supports EWMH. Will gracefully fallback if not available

    // https://specifications.freedesktop.org/wm-spec/wm-spec-1.5.html#idm45381391305328
    let wm_client_list = connection.send_request(&x::InternAtom {
        only_if_exists: true,
        name: b"_NET_CLIENT_LIST_STACKING",
    });

    // https://specifications.freedesktop.org/wm-spec/wm-spec-1.5.html#idm45381391244864
    let frame_extents = connection.send_request(&x::InternAtom {
        only_if_exists: true,
        name: b"_NET_FRAME_EXTENTS",
    });
    let wm_client_list = connection.wait_for_reply(wm_client_list)?;
    let frame_extents = connection.wait_for_reply(frame_extents)?;

    let wm_client_list = if wm_client_list.atom() != ATOM_NONE {
        wm_client_list.atom()
    } else {
        return Err(Error::WmDoesNotSupportEwmh);
    };
    let frame_extents = if frame_extents.atom() != ATOM_NONE {
        frame_extents.atom()
    } else {
        return Err(Error::WmDoesNotSupportEwmh);
    };

    for root_screen in setup.roots() {
        let root_window = root_screen.root();
        let pointer_cookie = connection.send_request(&x::QueryPointer {
            window: root_window,
        });

        let pointer_reply = connection.wait_for_reply(pointer_cookie)?;
        if pointer_reply.same_screen() {
            let list = connection.send_request(&x::GetProperty {
                delete: false,
                window: root_window,
                property: wm_client_list,
                r#type: ATOM_WINDOW,
                long_offset: 0,
                long_length: 128, // If the user has more than 128 windows on their desktop, that's their problem really
            });
            let list = connection.wait_for_reply(list)?;

            let mut windows = Vec::with_capacity(128);

            for xid in list.value::<u32>() {
                // SAFETY: We got this from the X server so it should be a valid resource ID, but if
                //         the server is lying to us, we can't do anything really.
                let window = unsafe { XWindow::new(*xid) };

                let attributes = connection.send_request(&x::GetWindowAttributes { window });
                let attributes = connection.wait_for_reply(attributes)?;
                if attributes.map_state() != MapState::Viewable {
                    continue;
                }

                let window_extents = connection.send_request(&shape::QueryExtents {
                    destination_window: window,
                });
                let window_extents = connection.wait_for_reply(window_extents)?;

                let translated_window_coords = connection.send_request(&x::TranslateCoordinates {
                    src_window: window,
                    dst_window: root_window,
                    src_x: window_extents.bounding_shape_extents_x(),
                    src_y: window_extents.bounding_shape_extents_y(),
                });

                let frame_extents = connection.send_request(&x::GetProperty {
                    delete: false,
                    window,
                    property: frame_extents,
                    r#type: ATOM_CARDINAL,
                    long_offset: 0,
                    long_length: 4,
                });

                // Batch requests when we can
                let frame_extents = connection.wait_for_reply(frame_extents)?;
                let translated_window_coords =
                    connection.wait_for_reply(translated_window_coords)?;

                let left = frame_extents.value::<u32>()[0];
                let right = frame_extents.value::<u32>()[1];
                let top = frame_extents.value::<u32>()[2];
                let bottom = frame_extents.value::<u32>()[3];

                windows.push(Window {
                    outer_rect: Rectangle {
                        x: translated_window_coords.dst_x() as f64 - left as f64,
                        y: translated_window_coords.dst_y() as f64 - top as f64,
                        // Above these lines we offsetted the content rect to the start of the window decorations
                        // as such, here we must grow the rect by how much we subtracted in order to cover the whole
                        // area of the window
                        w: window_extents.bounding_shape_extents_width() as f64
                            + (left + right) as f64,
                        h: window_extents.bounding_shape_extents_height() as f64
                            + (top + bottom) as f64,
                    },
                    content_rect: Rectangle {
                        x: translated_window_coords.dst_x() as f64,
                        y: translated_window_coords.dst_y() as f64,
                        w: window_extents.bounding_shape_extents_width() as f64,
                        h: window_extents.bounding_shape_extents_height() as f64,
                    },
                });
            }

            return Ok(windows);
        }
    }

    Err(Error::FailedToGetWindows)
}

fn find_xcb_visualtype(conn: &xcb::Connection, visual_id: u32) -> Option<x::Visualtype> {
    for root in conn.get_setup().roots() {
        for depth in root.allowed_depths() {
            for visual in depth.visuals() {
                if visual.visual_id() == visual_id {
                    return Some(*visual);
                }
            }
        }
    }

    None
}

/// Gets the screen resolution
///
/// # Returns
/// The first item of the tuple is the width, the second is the height
pub fn get_screen_resolution() -> Result<(i32, i32), Error> {
    let (connection, _) = xcb::Connection::connect(None)?;
    let setup = connection.get_setup();

    for root_screen in setup.roots() {
        let window = root_screen.root();
        let pointer_cookie = connection.send_request(&x::QueryPointer { window });
        let geometry_cookie = connection.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(window),
        });

        let pointer_reply = connection.wait_for_reply(pointer_cookie)?;
        if pointer_reply.same_screen() {
            let geometry = connection.wait_for_reply(geometry_cookie)?;
            return Ok((geometry.width() as i32, geometry.height() as i32));
        }
    }

    Err(Error::FailedToGetScreenResolution)
}
