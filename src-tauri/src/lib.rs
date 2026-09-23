pub mod commands;
pub mod domain;
pub mod infra;
#[cfg(test)]
mod data_policy;
#[cfg(test)]
mod testing;
pub mod services;

/// What of the window comes back. Not the frame: that is the config's
/// (tauri.macos.conf.json swaps it), and a restored one would outlive every
/// change to it.
#[cfg(desktop)]
fn window_state() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    StateFlags::all() & !StateFlags::DECORATIONS
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // First, as the plugin requires: plugins start in the order they were
    // added, and a second launch must be turned away before anything else
    // comes up — the index, the MCP servers.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    // Applied when the window is created — so the window opens where it was,
    // not at the config's size and then jumping.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().with_state_flags(window_state()).build());
    // The plugin writes the file only on a clean exit. `tauri dev` restarting
    // after a rebuild, a Ctrl+C, a crash: none is one, and the window came back
    // at whatever size the last clean exit saw. Leaving the window — for the
    // editor or the terminal that is about to restart it — and closing it
    // write it too.
    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        use tauri::Manager;
        use tauri_plugin_window_state::AppHandleExt;
        if matches!(event, tauri::WindowEvent::Focused(false) | tauri::WindowEvent::CloseRequested { .. }) {
            if let Err(e) = window.app_handle().save_window_state(window_state()) {
                eprintln!("window size and position not saved: {e}");
            }
        }
    });
    builder
        .plugin(tauri_plugin_opener::init())
        // The folder picker. A file chooser is the platform's dialog, not one
        // this app should draw.
        .plugin(tauri_plugin_dialog::init())
        // One resident piece of state, shared by every command that has to
        // reach a turn while it runs. `Arc` because a turn runs on a blocking
        // thread that outlives the command call that started it.
        .manage(std::sync::Arc::new(commands::chat::AgentState::default()))
        .manage(commands::git::GitWatch::default())
        // The MCP servers, kept running between turns.
        .manage(std::sync::Arc::new(services::mcp_servers::McpServers::new(std::sync::Arc::new(
            |config, cwd, cancelled| {
                let server = infra::mcp_stdio::StdioServer::start(config, cwd, cancelled)?;
                Ok(std::sync::Arc::new(server) as std::sync::Arc<dyn domain::mcp::McpClient>)
            },
        ))))
        // The index of the open folder, and the one embedding model every
        // folder shares. Built in `setup` because the model's location is
        // Tauri's to know: the resource directory of the installed app.
        .setup(|app| {
            use tauri::Manager;
            // Background processes the agent started; they outlive turns.
            // Here because what they report goes out through the app.
            app.manage(std::sync::Arc::new(infra::background::Processes::new(
                commands::processes::process_event_sink(app.handle()),
            )));
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
            commands::chat::workspace_branch,
            commands::git::git_changes,
            commands::git::git_totals,
            commands::git::git_stage,
            commands::git::git_unstage,
            commands::git::git_commit,
            commands::chat::workspace_recent,
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
            commands::chat::approval_restore,
            commands::chat::approval_remember_get,
            commands::chat::approval_remember_set,
            commands::chat::chat_set_mode,
            commands::chat::chat_context_usage,
            commands::chat_history::chat_list,
            commands::chat_history::chat_load,
            commands::chat_history::chat_save,
            commands::chat_history::chat_delete,
            commands::chat_history::chat_export,
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
            commands::mcp::mcp_server_connect,
            commands::hooks::hooks_config_get,
            commands::hooks::hooks_config_save,
            commands::processes::processes_list,
            commands::processes::process_stop,
            commands::skills::skills_set_enabled,
            commands::skills::skills_set_source_enabled,
            commands::skills::rules_list,
            commands::skills::rules_set_enabled,
            commands::tool_log::tool_log_query,
            commands::tool_log::tool_log_clear,
            commands::tool_log::tool_log_enabled_get,
            commands::tool_log::tool_log_enabled_set,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Servers are in process groups of their own, so the app's exit
            // does not take them along; most would notice their stdin closing
            // and exit, and this does not leave that to them.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                app.state::<std::sync::Arc<services::mcp_servers::McpServers>>().stop_all();
                app.state::<std::sync::Arc<infra::background::Processes>>().stop_all();
            }
        });
}
