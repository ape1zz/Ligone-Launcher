mod auth;
mod content;
mod downloader;
mod fabric;
mod forge;
mod instances;
mod jre;
mod launch;
mod mcmeta;
mod paths;
mod settings;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(auth::AuthState::default())
        .manage(downloader::InstanceState::default())
        .manage(jre::JavaState::default())
        .manage(launch::LaunchState::default())
        .setup(|app| {
            auth::load_env(app.handle());
            auth::restore_last_session(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || jre::ensure_java(&handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_login_offline,
            auth::auth_login_saved,
            auth::auth_saved_accounts,
            auth::auth_remove_saved,
            auth::auth_current,
            auth::auth_logout,
            auth::auth_login_ely,
            auth::auth_cancel_ely,
            auth::auth_login_ely_password,
            auth::auth_ely_refresh,
            settings::settings_get,
            settings::settings_set,
            settings::settings_open_home,
            instances::instance_list,
            instances::instance_create,
            instances::instance_remove,
            instances::instance_rename,
            instances::instance_open_folder,
            mcmeta::version_list,
            fabric::fabric_loader_versions,
            forge::forge_loader_versions,
            downloader::instance_download,
            downloader::instance_cancel_download,
            jre::java_status,
            launch::instance_launch,
            launch::instance_running,
            content::content_search,
            content::content_versions,
            content::content_install,
            content::content_installed,
            content::content_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
