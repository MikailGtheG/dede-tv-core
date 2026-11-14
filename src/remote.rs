//! Fail-closed remote-control policy. No remote event can request an admin view,
//! Kodi window, desktop, shell, or arbitrary command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    LiveTv,
    ChannelList,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKey {
    Ok,
    Back,
    Up,
    Down,
    ChannelUp,
    ChannelDown,
    Home,
    Menu,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    ShowChannels,
    HideChannels,
    MoveSelection(i8),
    TuneSelected,
    StepChannel(i8),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteController {
    screen: Screen,
}
impl Default for RemoteController {
    fn default() -> Self {
        Self {
            screen: Screen::LiveTv,
        }
    }
}
impl RemoteController {
    pub fn screen(&self) -> Screen {
        self.screen
    }
    pub fn handle(&mut self, key: RemoteKey) -> Action {
        use {Action as A, RemoteKey as K, Screen as S};
        match (self.screen, key) {
            (S::LiveTv, K::Ok) => {
                self.screen = S::ChannelList;
                A::ShowChannels
            }
            (S::ChannelList, K::Ok) => {
                self.screen = S::LiveTv;
                A::TuneSelected
            }
            (S::ChannelList, K::Back | K::Home) => {
                self.screen = S::LiveTv;
                A::HideChannels
            }
            (S::ChannelList, K::Up) => A::MoveSelection(-1),
            (S::ChannelList, K::Down) => A::MoveSelection(1),
            (S::LiveTv, K::ChannelUp) => A::StepChannel(1),
            (S::LiveTv, K::ChannelDown) => A::StepChannel(-1),
            (S::ChannelList, K::ChannelUp) => {
                self.screen = S::LiveTv;
                A::StepChannel(1)
            }
            (S::ChannelList, K::ChannelDown) => {
                self.screen = S::LiveTv;
                A::StepChannel(-1)
            }
            (S::LiveTv, K::Up | K::Down | K::Back | K::Home | K::Menu | K::Unknown)
            | (S::ChannelList, K::Menu | K::Unknown) => A::None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const KEYS: [RemoteKey; 9] = [
        RemoteKey::Ok,
        RemoteKey::Back,
        RemoteKey::Up,
        RemoteKey::Down,
        RemoteKey::ChannelUp,
        RemoteKey::ChannelDown,
        RemoteKey::Home,
        RemoteKey::Menu,
        RemoteKey::Unknown,
    ];
    #[test]
    fn all_remote_sequences_remain_within_tv_and_channel_list() {
        fn walk(controller: RemoteController, depth: usize) {
            if depth == 0 {
                return;
            }
            for key in KEYS {
                let mut next = controller;
                next.handle(key);
                assert!(matches!(
                    next.screen(),
                    Screen::LiveTv | Screen::ChannelList
                ));
                walk(next, depth - 1);
            }
        }
        walk(RemoteController::default(), 5);
    }
    #[test]
    fn remote_never_opens_admin() {
        for key in KEYS {
            let mut controller = RemoteController::default();
            let action = controller.handle(key);
            assert!(matches!(
                action,
                Action::None | Action::ShowChannels | Action::StepChannel(_)
            ));
        }
    }
    #[test]
    fn channel_buttons_close_overlay_and_step_channel() {
        for (key, expected) in [
            (RemoteKey::ChannelUp, Action::StepChannel(1)),
            (RemoteKey::ChannelDown, Action::StepChannel(-1)),
        ] {
            let mut controller = RemoteController::default();
            assert_eq!(controller.handle(RemoteKey::Ok), Action::ShowChannels);
            assert_eq!(controller.handle(key), expected);
            assert_eq!(controller.screen(), Screen::LiveTv);
        }
    }
    #[test]
    fn ok_selects_and_returns_to_video() {
        let mut controller = RemoteController::default();
        assert_eq!(controller.handle(RemoteKey::Ok), Action::ShowChannels);
        assert_eq!(controller.handle(RemoteKey::Down), Action::MoveSelection(1));
        assert_eq!(controller.handle(RemoteKey::Ok), Action::TuneSelected);
        assert_eq!(controller.screen(), Screen::LiveTv);
    }
}
