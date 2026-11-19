pub(crate) struct StorageAdapter<T>(pub T);
impl kestrel_core::settings::Storage for StorageAdapter<&dyn eframe::Storage> {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get_string(key)
    }
    fn set_string(&mut self, _key: &str, _value: String) {
        unreachable!("read-only adapter")
    }
}
impl kestrel_core::settings::Storage for StorageAdapter<&mut dyn eframe::Storage> {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get_string(key)
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.set_string(key, value);
    }
}
