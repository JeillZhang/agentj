/// Plugin lifecycle state — the Kernel 6-state machine (design §模块生命周期状态机)
/// extended with Loading/Disabled/Reloading (design §插件生命周期).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    Unloaded,
    Loading,
    Initializing,
    Ready,
    Disabled,
    Reloading,
    Stopping,
    Stopped,
    Failed,
}

pub struct PluginStateMachine {
    id: String,
    state: PluginState,
    last_error: Option<String>,
}

impl PluginStateMachine {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: PluginState::Unloaded,
            last_error: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn state(&self) -> PluginState {
        self.state
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.clone()
    }

    pub fn begin_loading(&mut self) {
        if matches!(self.state, PluginState::Unloaded) {
            self.state = PluginState::Loading;
        }
    }

    pub fn initialized(&mut self) {
        if matches!(self.state, PluginState::Loading | PluginState::Reloading) {
            self.state = PluginState::Ready;
        }
    }

    pub fn disable(&mut self) {
        if matches!(self.state, PluginState::Ready) {
            self.state = PluginState::Disabled;
        }
    }

    pub fn enable(&mut self) {
        if matches!(self.state, PluginState::Disabled) {
            self.state = PluginState::Ready;
        }
    }

    pub fn begin_reload(&mut self) {
        if matches!(self.state, PluginState::Ready) {
            self.state = PluginState::Reloading;
        }
    }

    pub fn reload_failed(&mut self) {
        if matches!(self.state, PluginState::Reloading) {
            self.state = PluginState::Ready;
            self.last_error = Some("reload failed".into());
        }
    }

    pub fn mark_failed(&mut self, err: &str) {
        self.state = PluginState::Failed;
        self.last_error = Some(err.into());
    }

    pub fn stopping(&mut self) {
        if matches!(
            self.state,
            PluginState::Ready
                | PluginState::Disabled
                | PluginState::Reloading
                | PluginState::Failed
        ) {
            self.state = PluginState::Stopping;
        }
    }

    pub fn stopped(&mut self) {
        if matches!(self.state, PluginState::Stopping) {
            self.state = PluginState::Stopped;
        }
    }
}
