//! A late Kodi PVR response must never authorize a different source type.
//! The test exercises the public controller API without a Kodi process.
use dede_tv_core::{
    kodi,
    tv::{Source, TvSession},
    VirtualChannels,
};

#[test]
fn pvr_ack_cannot_confirm_reassigned_virtual_channel() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);
    let stale_request = kodi::open_pvr_channel(17, 1);

    tv.replace_channels(vec![("TRT 1".into(), Source::Virtual)]);
    assert!(tv
        .confirm_pvr_request(&db, "TRT 1", &stale_request)
        .is_err());
    assert_eq!(db.last_channel().unwrap(), None);
}

#[test]
fn stale_pvr_ack_preserves_previously_confirmed_channel() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(
        vec![
            ("TRT 1".into(), Source::Pvr(17)),
            ("ATV".into(), Source::Pvr(18)),
        ],
        vec![],
    );
    let valid_request = kodi::open_pvr_channel(18, 1);
    tv.confirm_pvr_request(&db, "ATV", &valid_request)
        .unwrap();

    let stale_request = kodi::open_pvr_channel(17, 2);
    tv.replace_channels(vec![
        ("TRT 1".into(), Source::Virtual),
        ("ATV".into(), Source::Pvr(18)),
    ]);
    assert!(tv
        .confirm_pvr_request(&db, "TRT 1", &stale_request)
        .is_err());
    assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
}
