//! Integration regression: an in-flight PVR acknowledgement must not persist a
//! different channel after the administrator reassigns its Kodi channel ID.
use dede_tv_core::{
    remote::RemoteKey,
    tv::{Source, TvEvent, TvSession},
    VirtualChannels,
};

#[test]
fn stale_ack_after_source_reassignment_does_not_change_boot_resume() {
    let db = VirtualChannels::open(":memory:").unwrap();
    let mut tv = TvSession::new(vec![("TRT 1".into(), Source::Pvr(17))], vec![]);

    tv.handle(RemoteKey::Ok, &db, 0).unwrap();
    let (name, request) = match tv.handle(RemoteKey::Ok, &db, 0).unwrap() {
        TvEvent::Playback { channel, request } => (channel, request),
        _ => panic!("expected Kodi playback request"),
    };

    // The administrator replaces the underlying PVR source while Kodi is
    // still processing the old Player.Open request.
    tv.replace_channels(vec![("TRT 1".into(), Source::Pvr(18))]);
    assert!(tv.confirm_pvr_request(&db, &name, &request).is_err());
    assert_eq!(db.last_channel().unwrap(), None);

    // A fresh acknowledgement for the replacement source is allowed.
    let new_request = dede_tv_core::kodi::open_pvr_channel(18, 1);
    tv.confirm_pvr_request(&db, "TRT 1", &new_request).unwrap();
    assert_eq!(db.last_channel().unwrap().as_deref(), Some("pvr:18"));
}
