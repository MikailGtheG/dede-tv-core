//! Channel-picker selection is deliberately independent of Kodi windows.
//! Only logical channel IDs are returned; playback remains in the core.
use crate::remote::{Action, RemoteController, RemoteKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPicker {
    remote: RemoteController,
    channels: Vec<String>,
    selected: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEvent {
    None,
    Show { selected: usize },
    Hide,
    SelectionChanged { selected: usize },
    Tune(String),
}
impl ChannelPicker {
    pub fn new(channels: Vec<String>, active: Option<&str>) -> Self {
        let selected = active
            .and_then(|id| channels.iter().position(|c| c == id))
            .unwrap_or(0);
        Self {
            remote: RemoteController::default(),
            channels,
            selected,
        }
    }
    pub fn channels(&self) -> &[String] {
        &self.channels
    }
    pub fn selected(&self) -> Option<&str> {
        self.channels.get(self.selected).map(String::as_str)
    }
    pub fn screen(&self) -> crate::remote::Screen {
        self.remote.screen()
    }
    pub fn handle(&mut self, key: RemoteKey) -> PickerEvent {
        match self.remote.handle(key) {
            Action::ShowChannels => PickerEvent::Show {
                selected: self.selected,
            },
            Action::HideChannels => PickerEvent::Hide,
            Action::MoveSelection(delta) if !self.channels.is_empty() => {
                let len = self.channels.len();
                if delta < 0 {
                    self.selected = (self.selected + len - 1) % len;
                } else {
                    self.selected = (self.selected + 1) % len;
                }
                PickerEvent::SelectionChanged {
                    selected: self.selected,
                }
            }
            Action::TuneSelected => self
                .selected()
                .map(|id| PickerEvent::Tune(id.to_owned()))
                .unwrap_or(PickerEvent::Hide),
            Action::StepChannel(delta) if !self.channels.is_empty() => {
                let len = self.channels.len();
                if delta < 0 {
                    self.selected = (self.selected + len - 1) % len;
                } else {
                    self.selected = (self.selected + 1) % len;
                }
                PickerEvent::Tune(self.channels[self.selected].clone())
            }
            _ => PickerEvent::None,
        }
    }
    /// Refresh channel list after an administrator changes it. Empty lists are safe.
    pub fn replace_channels(&mut self, channels: Vec<String>) {
        // Preserve the selected occurrence when display names are duplicated.
        // Matching only the name would silently jump to the first duplicate.
        let current = self.selected().map(str::to_owned);
        let occurrence = current.as_ref().map(|name| {
            self.channels[..self.selected]
                .iter()
                .filter(|candidate| *candidate == name)
                .count()
        });
        self.selected = current
            .as_deref()
            .zip(occurrence)
            .and_then(|(name, nth)| {
                channels
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| candidate.as_str() == name)
                    .nth(nth)
                    .map(|(index, _)| index)
            })
            .unwrap_or(0);
        self.channels = channels;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn picker() -> ChannelPicker {
        ChannelPicker::new(vec!["TRT 1".into(), "Dede Film".into()], Some("TRT 1"))
    }
    #[test]
    fn ok_opens_picker_and_tunes_selected() {
        let mut p = picker();
        assert_eq!(p.handle(RemoteKey::Ok), PickerEvent::Show { selected: 0 });
        assert_eq!(
            p.handle(RemoteKey::Down),
            PickerEvent::SelectionChanged { selected: 1 }
        );
        assert_eq!(
            p.handle(RemoteKey::Ok),
            PickerEvent::Tune("Dede Film".into())
        );
        assert_eq!(p.screen(), crate::remote::Screen::LiveTv);
    }
    #[test]
    fn channel_buttons_wrap_without_opening_any_ui() {
        let mut p = picker();
        assert_eq!(
            p.handle(RemoteKey::ChannelDown),
            PickerEvent::Tune("Dede Film".into())
        );
        assert_eq!(
            p.handle(RemoteKey::ChannelUp),
            PickerEvent::Tune("TRT 1".into())
        );
        assert_eq!(p.handle(RemoteKey::Menu), PickerEvent::None);
    }
    #[test]
    fn empty_list_never_panics_or_tunes() {
        let mut p = ChannelPicker::new(vec![], None);
        for key in [
            RemoteKey::ChannelUp,
            RemoteKey::ChannelDown,
            RemoteKey::Ok,
            RemoteKey::Up,
            RemoteKey::Down,
            RemoteKey::Ok,
        ] {
            assert!(!matches!(p.handle(key), PickerEvent::Tune(_)));
        }
    }
    #[test]
    fn replacement_preserves_second_duplicate_occurrence() {
        let mut p = ChannelPicker::new(vec!["Same".into(), "Same".into(), "Other".into()], None);
        assert_eq!(
            p.handle(RemoteKey::ChannelUp),
            PickerEvent::Tune("Same".into())
        );
        p.replace_channels(vec!["Same".into(), "Other".into(), "Same".into()]);
        assert_eq!(p.handle(RemoteKey::Ok), PickerEvent::Show { selected: 2 });
    }

    #[test]
    fn replacement_preserves_selection_if_possible() {
        let mut p = picker();
        p.handle(RemoteKey::ChannelUp);
        p.replace_channels(vec!["New".into(), "Dede Film".into()]);
        assert_eq!(p.selected(), Some("Dede Film"));
        p.replace_channels(vec![]);
        assert_eq!(p.selected(), None);
    }
}
