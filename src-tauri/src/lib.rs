mod config;
mod detect;
mod devices;
mod env_path;
mod github;
mod jobs;
mod proc;
mod requirements;
mod secrets;
mod share;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_path::fix_environment();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(jobs::JobManager::default())
        .invoke_handler(tauri::generate_handler![
            config::config_load,
            config::config_save,
            config::path_exists,
            config::write_text_file,
            detect::detect_project,
            devices::list_devices,
            requirements::check_tools,
            requirements::system_info,
            jobs::job_start,
            jobs::job_input,
            jobs::job_resize,
            jobs::job_cancel,
            share::share_start,
            share::share_stop,
            share::share_list,
            secrets::secret_set,
            secrets::secret_has,
            secrets::secret_delete,
            github::gh_connect_token,
            github::gh_import_cli,
            github::gh_status,
            github::gh_logout,
            github::gh_repos,
            github::gh_workflows,
            github::gh_runs,
            github::gh_dispatch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
