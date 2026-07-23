use super::*;
pub(in crate::app) struct AuxiliaryWindowSpec {
    pub(in crate::app) slot: AuxiliaryWindowSlot,
    pub(in crate::app) title: SharedString,
    pub(in crate::app) size: gpui::Size<Pixels>,
    pub(in crate::app) min_size: gpui::Size<Pixels>,
    pub(in crate::app) already_open_message: &'static str,
    pub(in crate::app) opened_message: &'static str,
    pub(in crate::app) failed_prefix: &'static str,
}

#[derive(Clone, Copy)]
pub(in crate::app) enum AuxiliaryWindowSlot {
    Settings,
    About,
    UpdateDialog,
    GitClone,
    GitCredentials,
    MemoryManager,
    ProjectEditor,
    WorktreeCreator,
    SshProfileEditor,
    DbProfileEditor,
    DbProfileShare,
    FilePicker,
}

mod commands;
mod keyboard;
mod menu;
mod view_toggles;
mod windows;
