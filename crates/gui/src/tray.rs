//! Tray icon through StatusNotifierItem over D-Bus (ksni), ADR-0004.

use arch_health_core::Status;
use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip};

use crate::style;

#[derive(Debug, Clone, Copy)]
pub enum TrayCommand {
    Open,
    RunNow,
    Quit,
}

pub struct HealthTray {
    pub status: Status,
    pub summary: String,
    pub running: bool,
    tx: async_channel::Sender<TrayCommand>,
}

impl HealthTray {
    pub fn new(tx: async_channel::Sender<TrayCommand>) -> Self {
        Self {
            status: Status::Ok,
            summary: "Waiting for the first check…".into(),
            running: false,
            tx,
        }
    }

    fn send(&self, cmd: TrayCommand) {
        // The receiver lives as long as the application.
        let _ = self.tx.send_blocking(cmd);
    }
}

impl ksni::Tray for HealthTray {
    fn id(&self) -> String {
        "arch-health".into()
    }

    fn title(&self) -> String {
        "arch-health".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn status(&self) -> ksni::Status {
        // NeedsAttention keeps the icon visible instead of in the overflow.
        if self.status >= Status::Warning {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn icon_name(&self) -> String {
        style::tray_icon(self.status).into()
    }

    fn attention_icon_name(&self) -> String {
        style::tray_icon(self.status).into()
    }

    fn tool_tip(&self) -> ToolTip {
        let description = if self.running {
            "Running checks…".to_string()
        } else {
            self.summary.clone()
        };
        ToolTip {
            title: format!("arch-health: {}", style::headline(self.status)),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open".into(),
                icon_name: "window-new".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Open)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Run checks now".into(),
                icon_name: "view-refresh".into(),
                enabled: !self.running,
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::RunNow)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
