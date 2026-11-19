pub use kestrel_core::branding::{APP_ID, APP_NAME};
pub fn migrate_storage() {
    let Some(current_dir) = eframe::storage_dir(APP_ID) else {
        return;
    };
    let legacy_dirs = kestrel_core::branding::LEGACY_APP_IDS
        .iter()
        .filter_map(|app_id| eframe::storage_dir(app_id))
        .collect::<Vec<_>>();
    let _ = kestrel_core::branding::migrate_storage_from_dirs(&current_dir, &legacy_dirs);
}
