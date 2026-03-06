

#[derive(Clone)]
pub struct ConfigField {
    pub key: &'static str,
    pub lua_key: &'static str,
    pub value: String,
    pub default: String,
    pub options: Vec<&'static str>,
    pub skip_write: bool,
}

impl ConfigField {
    pub fn has_options(&self) -> bool {
        !self.options.is_empty()
    }
}
