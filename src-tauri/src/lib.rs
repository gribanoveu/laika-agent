pub mod commands;
pub mod domain;
pub mod infra;
#[cfg(test)]
mod testing;
pub mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // The folder picker. A file chooser is the platform's dialog, not one
        // this app should draw.
        .plugin(tauri_plugin_dialog::init())
        // One resident piece of state, shared by every command that has to
        // reach a turn while it runs. `Arc` because a turn runs on a blocking
        // thread that outlives the command call that started it.
        .manage(std::sync::Arc::new(commands::chat::AgentState::default()))
        // The index of the open folder, and the one embedding model every
        // folder shares. Built in `setup` because the model's location is
        // Tauri's to know: the resource directory of the installed app.
        .setup(|app| {
            use tauri::Manager;
            let resources = app.path().resource_dir().ok();
            let model = infra::local_embeddings::LocalEmbeddings::new(
                infra::local_embeddings::bundled_model_dir(resources.as_deref()),
                infra::local_embeddings::DEFAULT_IDLE_UNLOAD,
            );
            let index_dir = infra::app_dir::ensure()?.join("index");
            app.manage(std::sync::Arc::new(services::workspace_index::WorkspaceIndex::new(
                index_dir,
                std::sync::Arc::new(model),
            )));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::workspace_open,
            commands::chat::workspace_current,
            commands::chat::workspace_index_status,
            commands::chat::chat_start,
            commands::chat::chat_resume,
            commands::chat::chat_cancel,
            commands::chat::chat_preview,
            commands::chat::chat_compact,
            commands::chat::chat_steer,
            commands::chat::chat_cancel_steer,
            commands::chat::approval_always_allow,
            commands::chat::approval_set_unattended,
            commands::chat::chat_set_mode,
            commands::chat::chat_context_usage,
            commands::chat_history::chat_list,
            commands::chat_history::chat_load,
            commands::chat_history::chat_save,
            commands::chat_history::chat_delete,
            commands::settings::llm_settings_get,
            commands::settings::llm_provider_save,
            commands::settings::llm_provider_remove,
            commands::settings::llm_api_key_save,
            commands::settings::llm_active_provider_set,
            commands::settings::llm_debug_logging_set,
            commands::settings::llm_models_list,
            commands::settings::agent_readiness,
            commands::skills::skills_list,
            commands::mcp::mcp_config_get,
            commands::mcp::mcp_config_save,
            commands::mcp::mcp_server_set_enabled,
            commands::skills::skills_set_enabled,
            commands::skills::rules_list,
            commands::skills::rules_set_enabled,
            commands::tool_log::tool_log_query,
            commands::tool_log::tool_log_clear,
            commands::tool_log::tool_log_enabled_get,
            commands::tool_log::tool_log_enabled_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
