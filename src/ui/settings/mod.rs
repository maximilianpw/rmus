pub mod account;
pub mod settings_panel;
pub mod sources;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Account,
    Keybinds,
}

impl SettingsTab {
    pub const ALL: &'static [SettingsTab] = &[
        SettingsTab::General,
        SettingsTab::Account,
        SettingsTab::Keybinds,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Account => "Account",
            SettingsTab::Keybinds => "Keybinds",
        }
    }
}
