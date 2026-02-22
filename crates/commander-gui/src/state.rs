use anyhow::Result;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct GuiState {
    pub store: Arc<StateStore>,
    pub tmux: Option<Arc<TmuxOrchestrator>>,
    pub current_session: Arc<RwLock<Option<String>>>,
    pub bot_status: Arc<RwLock<DaemonStatus>>,
}

#[derive(Clone, Debug)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
}

impl GuiState {
    pub fn new() -> Result<Self> {
        // Use commander_core::config to get the state directory
        let state_dir = commander_core::config::state_dir();
        let store = StateStore::new(state_dir);
        let tmux = TmuxOrchestrator::new().ok();

        Ok(Self {
            store: Arc::new(store),
            tmux: tmux.map(Arc::new),
            current_session: Arc::new(RwLock::new(None)),
            bot_status: Arc::new(RwLock::new(DaemonStatus {
                running: false,
                pid: None,
            })),
        })
    }
}
