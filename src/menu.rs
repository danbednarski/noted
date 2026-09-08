//! Native menu bar (muda): the app menu and a Theme submenu.

use crate::theme::ThemeKind;
use muda::{CheckMenuItem, Menu, MenuEvent, PredefinedMenuItem, Submenu};
use std::sync::mpsc::Receiver;

pub struct AppMenu {
    _menu: Menu,
    themes: [(ThemeKind, CheckMenuItem); 3],
    events: Receiver<MenuEvent>,
}

impl AppMenu {
    pub fn install(ctx: &egui::Context, initial: ThemeKind) -> Self {
        let menu = Menu::new();

        let app_menu = Submenu::new("noted", true);
        let _ = app_menu.append_items(&[
            &PredefinedMenuItem::about(Some("About noted"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);

        let themes = [
            (ThemeKind::CandyCane, "Candy Cane"),
            (ThemeKind::Win95, "Windows 95"),
            (ThemeKind::Dracula, "Dracula"),
        ]
        .map(|(kind, label)| (kind, CheckMenuItem::new(label, true, initial == kind, None)));
        let theme_menu = Submenu::new("Theme", true);
        for (_, item) in &themes {
            let _ = theme_menu.append(item);
        }

        let _ = menu.append_items(&[&app_menu, &theme_menu]);

        #[cfg(target_os = "macos")]
        menu.init_for_nsapp();

        // Bridge muda's global menu events to our app + wake the egui loop.
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx2.request_repaint();
        }));

        Self {
            _menu: menu,
            themes,
            events: rx,
        }
    }

    /// Drains menu events. Returns the theme the user picked, if any, with the
    /// check marks already moved.
    pub fn poll(&mut self) -> Option<ThemeKind> {
        let mut picked = None;
        while let Ok(ev) = self.events.try_recv() {
            if let Some((kind, _)) = self.themes.iter().find(|(_, item)| *item.id() == ev.id) {
                picked = Some(*kind);
            }
        }
        if let Some(t) = picked {
            for (kind, item) in &self.themes {
                item.set_checked(*kind == t);
            }
        }
        picked
    }
}
