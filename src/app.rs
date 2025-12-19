use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
};

use crate::{
    config::Config,
    sources::local::LocalFiles,
    ui::{center_panel::CenterPanel, left_panel::LeftPanel, right_panel::RightPanel},
};

use crate::event::handle_crossterm_events;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum FocusedWindow {
    #[default]
    Left,
    Center,
    Right,
}

impl FocusedWindow {
    pub fn next(&self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub config: Config,
    pub running: bool,
    pub focused_window: FocusedWindow,
    pub left_panel: LeftPanel,
    pub center_panel: CenterPanel,
    pub right_panel: RightPanel,
}

impl App {
    pub fn new() -> Self {
        Self {
            config: Config::load(),
            left_panel: LeftPanel::new(vec![LocalFiles::new()]),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(),
            ..Default::default()
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            handle_crossterm_events(&mut self)?;
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Percentage(60),
            Constraint::Fill(1),
        ]);
        let [left_area, center_area, right_area] = layout.areas(frame.area());

        self.left_panel
            .render(frame, left_area, self.focused_window == FocusedWindow::Left);
        self.center_panel.render(
            frame,
            center_area,
            self.focused_window == FocusedWindow::Center,
        );
        self.right_panel.render(
            frame,
            right_area,
            self.focused_window == FocusedWindow::Right,
        );
    }

    pub(crate) fn quit(&mut self) {
        self.running = false;
    }
}
