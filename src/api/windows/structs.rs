use std::sync::mpsc::Sender;
use winapi::shared::{
    minwindef::HINSTANCE,
    windef::{HMENU, HWND},
};

#[derive(Clone)]
pub(crate) struct WindowInfo {
    pub hwnd: HWND,
    pub hinstance: HINSTANCE,
    pub hmenu: HMENU,
}

unsafe impl Send for WindowInfo {}
unsafe impl Sync for WindowInfo {}

#[derive(Clone)]
pub(crate) struct WindowsLoopData {
    pub info: WindowInfo,
    pub tx: Sender<WindowsTrayEvent>,
    pub left_click_tx: Sender<LeftClickCallbackEvent>,
}

pub(crate) struct WindowsTrayEvent(pub(crate) u32);

pub enum LeftClickCallbackEvent {
    Click,
    Quit,
}
