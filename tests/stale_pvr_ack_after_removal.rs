//! A delayed Kodi acknowledgement must not revive a channel removed by an administrator.
//! This is a pure integration regression: no Kodi process is required.
use dede_tv_core::{
    kodi,
    tv::{Source, TvSession},
    VirtualChannels,
};

#[test]
fn removed_channel_cannot_be_persisted_by_late_pvr_ack() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
    let request = kodi::open_pvr_channel(17, 1);

    tv.replace_channels(vec![]);
    assert!(tv.confirm_pvr_request(&db, "TRT 1", &request).is_err());
    assert_eq!(db.last_channel().unwrap(), None);
}

#[test]
fn late_ack_cannot_overwrite_previous_confirmed_resume_target() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(
        vec![
            ("TRT 1".into(), Source::Pvr(17)),
            ("ATV".into(), Source::Pvr(18)),
        ],
        vec![],
    );
    let previous = kodi::open_pvr_channel(18, 1);
    tv.confirm_pvr_request(&db, "ATV", &previous).unwrap();

    let stale = kodi::open_pvr_channel(17, 2);
    tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(19))]);
    assert!(tv.confirm_pvr_request(&db, "TRT 1", &stale).is_err());
    assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
}
