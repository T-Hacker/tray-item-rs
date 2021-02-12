// Most of this code is taken from https://github.com/qdot/systray-rs/blob/master/src/api/win32/mod.rs
// Open source is great :)

use crate::TIError;
use std::{
    self,
    cell::RefCell,
    sync::{
        mpsc::{channel, Sender},
        Arc, Mutex,
    },
    thread,
};
use winapi::{
    shared::{
        minwindef::{LPARAM, WPARAM},
        windef::HICON,
    },
    um::{
        shellapi::{self, NIF_ICON, NIF_TIP, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW},
        winuser::{
            self, IMAGE_ICON, MENUITEMINFOW, MFS_DISABLED, MFS_UNHILITE, MFT_STRING, MIIM_FTYPE,
            MIIM_ID, MIIM_STATE, MIIM_STRING, WM_DESTROY,
        },
    },
};

mod funcs;
mod structs;
use funcs::*;
use structs::*;

thread_local!(static WININFO_STASH: RefCell<Option<WindowsLoopData>> = RefCell::new(None));

type CallBackEntry = Option<Box<dyn Fn() -> () + Send + Sync + 'static>>;

pub struct TrayItemWindows {
    entries: Arc<Mutex<Vec<CallBackEntry>>>,
    left_click_callback: Arc<Mutex<CallBackEntry>>,
    info: WindowInfo,
    windows_loop: Option<thread::JoinHandle<()>>,
    event_loop: Option<thread::JoinHandle<()>>,
    event_tx: Sender<WindowsTrayEvent>,

    left_click_loop: Option<thread::JoinHandle<()>>,
    left_click_tx: Sender<LeftClickCallbackEvent>,
}

impl TrayItemWindows {
    pub fn new(title: &str, icon: Option<&str>) -> Result<Self, TIError> {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel();
        let (event_tx, event_rx) = channel::<WindowsTrayEvent>();

        let left_click_callback = Arc::new(Mutex::new(None));
        let (left_click_tx, left_click_rx) = channel::<LeftClickCallbackEvent>();

        let entries_clone = Arc::clone(&entries);
        let event_loop = thread::spawn(move || loop {
            match event_rx.recv() {
                Ok(v) => {
                    if v.0 == u32::MAX {
                        break;
                    }

                    padlock::mutex_lock(&entries_clone, |ents: &mut Vec<CallBackEntry>| match &ents
                        [v.0 as usize]
                    {
                        Some(f) => f(),
                        None => (),
                    })
                }

                Err(_) => (),
            }
        });

        let left_click_callback_clone = Arc::clone(&left_click_callback);
        let left_click_loop = thread::spawn(move || loop {
            if let Ok(event) = left_click_rx.recv() {
                match event {
                    LeftClickCallbackEvent::Click => {
                        padlock::mutex_lock(
                            &left_click_callback_clone,
                            |cb: &mut CallBackEntry| {
                                if let Some(cb) = cb {
                                    cb();
                                }
                            },
                        );
                    }
                    LeftClickCallbackEvent::Quit => break,
                }
            }
        });

        let event_tx_clone = event_tx.clone();
        let left_click_tx_clone = left_click_tx.clone();
        let windows_loop = thread::spawn(move || unsafe {
            let i = init_window();
            let k;

            match i {
                Ok(j) => {
                    tx.send(Ok(j.clone())).ok();
                    k = j;
                }

                Err(e) => {
                    tx.send(Err(e)).ok();
                    return;
                }
            }

            WININFO_STASH.with(|stash| {
                let data = WindowsLoopData {
                    info: k,
                    tx: event_tx_clone,
                    left_click_tx: left_click_tx_clone,
                };

                (*stash.borrow_mut()) = Some(data);
            });

            run_loop();
        });

        let info = match rx.recv().unwrap() {
            Ok(i) => i,
            Err(e) => return Err(e),
        };

        let w = Self {
            entries: entries,
            left_click_callback,
            info: info,
            windows_loop: Some(windows_loop),
            event_loop: Some(event_loop),
            event_tx: event_tx,

            left_click_loop: Some(left_click_loop),
            left_click_tx,
        };

        w.set_tooltip(title)?;
        w.set_icon(icon)?;

        Ok(w)
    }

    pub fn set_icon(&self, icon: Option<&str>) -> Result<(), TIError> {
        self.set_icon_from_resource(icon)
    }

    pub fn add_label(&mut self, label: &str) -> Result<(), TIError> {
        let item_idx = padlock::mutex_lock(&self.entries, |entries| {
            let len = entries.len();
            entries.push(None);
            len
        }) as u32;

        let mut st = to_wstring(label);
        let mut item = get_menu_item_struct();
        item.fMask = MIIM_FTYPE | MIIM_STRING | MIIM_ID | MIIM_STATE;
        item.fType = MFT_STRING;
        item.fState = MFS_DISABLED | MFS_UNHILITE;
        item.wID = item_idx;
        item.dwTypeData = st.as_mut_ptr();
        item.cch = (label.len() * 2) as u32;
        unsafe {
            if winuser::InsertMenuItemW(self.info.hmenu, item_idx, 1, &item as *const MENUITEMINFOW)
                == 0
            {
                return Err(get_win_os_error("Error inserting menu item"));
            }
        }
        Ok(())
    }

    pub fn add_menu_item<F>(&mut self, label: &str, cb: F) -> Result<(), TIError>
    where
        F: Fn() -> () + Send + Sync + 'static,
    {
        let item_idx = padlock::mutex_lock(&self.entries, |entries| {
            let len = entries.len();
            entries.push(Some(Box::new(cb)));
            len
        }) as u32;

        let mut st = to_wstring(label);
        let mut item = get_menu_item_struct();
        item.fMask = MIIM_FTYPE | MIIM_STRING | MIIM_ID | MIIM_STATE;
        item.fType = MFT_STRING;
        item.wID = item_idx;
        item.dwTypeData = st.as_mut_ptr();
        item.cch = (label.len() * 2) as u32;
        unsafe {
            if winuser::InsertMenuItemW(self.info.hmenu, item_idx, 1, &item as *const MENUITEMINFOW)
                == 0
            {
                return Err(get_win_os_error("Error inserting menu item"));
            }
        }
        Ok(())
    }

    pub fn set_left_click_callback<F>(&mut self, cb: Option<F>)
    where
        F: Fn() -> () + Send + Sync + 'static,
    {
        let mut left_click_callback = self.left_click_callback.lock().unwrap();
        if let Some(cb) = cb {
            *left_click_callback = Some(Box::new(cb));
        } else {
            *left_click_callback = None;
        }
    }

    // others

    fn set_tooltip(&self, tooltip: &str) -> Result<(), TIError> {
        // Add Tooltip
        // Gross way to convert String to [i8; 128]
        // TODO: Clean up conversion, test for length so we don't panic at runtime
        let tt = tooltip.as_bytes().clone();
        let mut nid = get_nid_struct(&self.info.hwnd);
        for i in 0..tt.len() {
            nid.szTip[i] = tt[i] as u16;
        }
        nid.uFlags = NIF_TIP;
        unsafe {
            if shellapi::Shell_NotifyIconW(NIM_MODIFY, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error setting tooltip"));
            }
        }
        Ok(())
    }

    fn set_icon_from_resource(&self, resource_name: Option<&str>) -> Result<(), TIError> {
        let resource_name = if let Some(resource_name) = resource_name {
            to_wstring(&resource_name).as_ptr()
        } else {
            1 as *const u16
        };

        let icon;
        unsafe {
            icon = winuser::LoadImageW(self.info.hinstance, resource_name, IMAGE_ICON, 64, 64, 0)
                as HICON;
            if icon == std::ptr::null_mut() as HICON {
                return Err(get_win_os_error("Error setting icon from resource"));
            }
        }
        self._set_icon(icon)
    }

    fn _set_icon(&self, icon: HICON) -> Result<(), TIError> {
        unsafe {
            let mut nid = get_nid_struct(&self.info.hwnd);
            nid.uFlags = NIF_ICON;
            nid.hIcon = icon;
            if shellapi::Shell_NotifyIconW(NIM_MODIFY, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error setting icon"));
            }
        }
        Ok(())
    }

    pub fn quit(&mut self) {
        unsafe {
            winuser::PostMessageW(self.info.hwnd, WM_DESTROY, 0 as WPARAM, 0 as LPARAM);
        }
        if let Some(t) = self.windows_loop.take() {
            t.join().ok();
        }
        if let Some(t) = self.event_loop.take() {
            self.event_tx.send(WindowsTrayEvent(u32::MAX)).ok();
            t.join().ok();
        }
        if let Some(t) = self.left_click_loop.take() {
            self.left_click_tx.send(LeftClickCallbackEvent::Quit).ok();
            t.join().ok();
        }
    }

    pub fn shutdown(&self) -> Result<(), TIError> {
        unsafe {
            let mut nid = get_nid_struct(&self.info.hwnd);
            nid.uFlags = NIF_ICON;
            if shellapi::Shell_NotifyIconW(NIM_DELETE, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error deleting icon from menu"));
            }
        }

        Ok(())
    }
}

impl Drop for TrayItemWindows {
    fn drop(&mut self) {
        self.shutdown().ok();
        self.quit();
    }
}
