mod api;
mod error;
pub use error::TIError;

pub struct TrayItem(api::TrayItemImpl);

impl TrayItem {
    pub fn new(title: &str, icon: Option<&str>) -> Result<Self, TIError> {
        Ok(Self(api::TrayItemImpl::new(title, icon)?))
    }

    pub fn set_icon(&mut self, icon: Option<&str>) -> Result<(), TIError> {
        self.0.set_icon(icon)
    }

    pub fn add_label(&mut self, label: &str) -> Result<(), TIError> {
        self.0.add_label(label)
    }

    pub fn add_menu_item<F>(&mut self, label: &str, cb: F) -> Result<(), TIError>
    where
        F: Fn() -> () + Send + Sync + 'static,
    {
        self.0.add_menu_item(label, cb)
    }

    pub fn inner_mut(&mut self) -> &mut api::TrayItemImpl {
        &mut self.0
    }

    pub fn set_left_click_callback<F>(&mut self, cb: Option<F>)
    where
        F: Fn() -> () + Send + Sync + 'static,
    {
        self.0.set_left_click_callback(cb)
    }
}
