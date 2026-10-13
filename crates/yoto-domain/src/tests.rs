use crate::*;
#[test]
fn bounds_cannot_be_bypassed_by_json() {
    assert!(serde_json::from_str::<MediaTime>("-1").is_err());
    assert!(serde_json::from_str::<Volume>("101").is_err());
    assert!(serde_json::from_str::<TrimRange>("[5000000,1000000]").is_err());
    let trim: TrimRange = serde_json::from_str("[1000000,8000000]").unwrap();
    assert!(
        TrimRange::new(
            trim.start(),
            trim.end(),
            MediaTime::from_seconds(6.).unwrap()
        )
        .is_err()
    );
    assert!(serde_json::from_str::<LocalSha256>("\"remote-media-id\"").is_err());
}
#[test]
fn vocabulary_is_closed_and_exhaustive() {
    for value in Scope::ALL {
        let parsed: Scope = serde_json::from_value(serde_json::json!(value.wire())).unwrap();
        assert_eq!(*value, parsed);
    }
    assert!(serde_json::from_str::<Autoadvance>("\"random-new-server-value\"").is_err());
}
#[test]
fn movement_keeps_remote_keys() {
    let mut p = Playlist::new("Test".into()).unwrap();
    let track = |name: &str| Track {
        events: None,
        id: TrackId::new(),
        key: TrackKey::new(name).unwrap(),
        title: name.into(),
        icon: None,
        media: TrackMedia::Stream {
            url: "https://example.com/audio.mp3".into(),
            format: AudioFormat::Mp3,
        },
        extra: Extra::new(),
    };
    let a = track("a");
    let aid = a.id;
    let b = track("b");
    let bid = b.id;
    let c = ChapterId::new();
    p.chapters.push(Chapter {
        id: c,
        key: ChapterKey::new("chapter").unwrap(),
        title: "Chapter".into(),
        icon: None,
        tracks: vec![a, b],
        extra: Extra::new(),
    });
    p.move_track(aid, c, 1).unwrap();
    assert_eq!(p.chapters[0].tracks[0].id, bid);
    assert_eq!(p.chapters[0].tracks[1].key.as_str(), "a");
    p.ungroup(c).unwrap();
    p.validate().unwrap();
    assert_eq!(p.chapters.len(), 2);
    assert_eq!(p.chapters[1].tracks[0].key.as_str(), "a");
}
#[test]
fn wire_key_namespaces_are_chapter_scoped_and_colliding_moves_are_atomic() {
    let mut p = Playlist::new("Chansons".into()).unwrap();
    for key in ["1", "2"] {
        p.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(key).unwrap(),
            title: format!("Chapitre {key}"),
            icon: None,
            tracks: vec![Track {
                id: TrackId::new(),
                key: TrackKey::new("1").unwrap(),
                title: "Une chanson".into(),
                icon: None,
                events: None,
                media: TrackMedia::Stream {
                    url: "https://example.com/1.mp3".into(),
                    format: AudioFormat::Mp3,
                },
                extra: Extra::new(),
            }],
            extra: Extra::new(),
        });
    }
    // Chapter "1" and its track "1" are different namespaces. Track "1" in
    // chapter "2" is also a distinct path, with its own local UUID.
    p.validate().unwrap();
    assert_ne!(p.chapters[0].tracks[0].id, p.chapters[1].tracks[0].id);
    let before = serde_json::to_value(&p).unwrap();
    assert!(matches!(
        p.move_track(p.chapters[0].tracks[0].id, p.chapters[1].id, 1),
        Err(Failure::Compatibility(_))
    ));
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
    let mut duplicate = p.chapters[0].tracks[0].clone();
    duplicate.id = TrackId::new();
    p.chapters[0].tracks.push(duplicate);
    assert!(
        matches!(p.validate(),Err(Failure::Validation(message)) if message=="Duplicate track key")
    );
}
#[test]
fn associations_are_account_and_recipe_specific() {
    let a = AccountId::new("a").unwrap();
    let b = AccountId::new("b").unwrap();
    let recipe = RecipeId::new();
    let modified = RecipeId::new();
    let map = PublicationMapping {
        account: a.clone(),
        recipe,
        digest: LocalSha256::new("ab".repeat(32)).unwrap(),
        media: YotoMediaId::new("opaque-remote").unwrap(),
        info: AudioInfo {
            duration: MediaTime::from_seconds(3.).unwrap(),
            bytes: 30,
            codec: "mp3".into(),
            sample_rate: 44100,
            channels: 2,
        },
    };
    let json = serde_json::to_string(&map).unwrap();
    let persisted: PublicationMapping = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(MediaSyncState::project(&a,recipe,std::slice::from_ref(&persisted),&[]),MediaSyncState::Uploaded{media} if media.as_str()=="opaque-remote")
    );
    assert!(matches!(
        MediaSyncState::project(&b, recipe, std::slice::from_ref(&persisted), &[]),
        MediaSyncState::LocalOnly
    ));
    assert!(matches!(
        MediaSyncState::project(&a, modified, &[persisted], &[]),
        MediaSyncState::LocalOnly
    ));
}
#[test]
fn shuffle_targets_and_wire_keys_survive_safe_moves_and_copy_and_gate_unsafe_edits() {
    let mut p = Playlist::new("Rich".into()).unwrap();
    for name in ["z", "a", "m"] {
        p.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(name).unwrap(),
            title: name.into(),
            icon: None,
            tracks: vec![Track {
                id: TrackId::new(),
                key: TrackKey::new(format!("track-{name}")).unwrap(),
                title: name.into(),
                icon: None,
                events: None,
                media: TrackMedia::Stream {
                    url: "https://example.com/audio.mp3".into(),
                    format: AudioFormat::Mp3,
                },
                extra: Extra::new(),
            }],
            extra: Extra::new(),
        });
    }
    p.playback.shuffle = vec![ShuffleGroup {
        start: 0,
        end: 1,
        limit: 1,
    }];
    let z = p.chapters[0].id;
    p.move_chapter(z, 1).unwrap();
    assert_eq!(p.playback.shuffle[0].start, 0);
    assert_eq!(p.chapters[1].id, z);
    let before = serde_json::to_value(&p).unwrap();
    assert!(p.move_chapter(z, 1).is_err());
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
    let first = p.chapters[0].tracks[0].id;
    assert!(p.remove_track(first).is_err());
    let mut event = Event::new(EventCommand::Goto);
    event.params = Some(std::collections::HashMap::from([(
        "trackKey".into(),
        serde_json::json!("track-m"),
    )]));
    let mut events = TrackEvents::new();
    events.on_end = Some(Box::new(event));
    p.chapters[0].tracks[0].events = Some(events);
    let copy = p.duplicate().unwrap();
    assert_ne!(copy.chapters[0].id, p.chapters[0].id);
    assert_eq!(copy.chapters[0].key, p.chapters[0].key);
    assert_eq!(copy.chapters[0].tracks[0].key, p.chapters[0].tracks[0].key);
    assert_eq!(
        copy.chapters[0].tracks[0].events,
        p.chapters[0].tracks[0].events
    );
    assert!(p.move_chapter(z, -1).is_err());
}
