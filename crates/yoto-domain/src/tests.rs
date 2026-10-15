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
    // Removing a chapter inside a shuffle range shrinks the range; emptying it drops it.
    let first = p.chapters[0].tracks[0].id;
    let mut removed = p.clone();
    removed.remove_track(first).unwrap();
    assert_eq!(removed.chapters.len(), 2);
    let range = &removed.playback.shuffle[0];
    assert_eq!((range.start, range.end, range.limit), (0, 0, 1));
    removed.validate().unwrap();
    let second = removed.chapters[0].tracks[0].id;
    removed.remove_track(second).unwrap();
    assert!(removed.playback.shuffle.is_empty());
    removed.validate().unwrap();
    assert!(p.remove_track(TrackId::new()).is_err());
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
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

#[test]
fn messages_quote_the_current_limits() {
    assert_eq!(
        messages::TITLE_LENGTH,
        format!("Title must contain 1–{} characters.", Rules::TITLE_MAX)
    );
    assert_eq!(
        messages::PLAYLIST_TRACKS_MAX,
        format!("A playlist supports at most {} tracks.", Rules::TRACKS_MAX)
    );
}
#[test]
fn long_ranges_are_cut_into_equal_contiguous_parts() {
    let minutes = |m: u64| MediaTime::try_from(m * 60_000_000).unwrap();
    let range = |a: u64, b: u64| TrimRange::new(minutes(a), minutes(b), minutes(b)).unwrap();
    assert_eq!(plan_parts(range(0, 30)), vec![range(0, 30)]);
    assert_eq!(plan_parts(range(0, 55)), vec![range(0, 55)]);
    let just_over = TrimRange::new(
        MediaTime::ZERO,
        MediaTime::try_from(Rules::PART_MICROS_MAX + 1).unwrap(),
        minutes(60),
    )
    .unwrap();
    assert_eq!(plan_parts(just_over).len(), 2);
    assert_eq!(
        plan_parts(range(0, 120)),
        vec![range(0, 40), range(40, 80), range(80, 120)]
    );
    // An odd length starting mid-file: parts touch, cover it exactly and stay short enough.
    let odd = TrimRange::new(
        MediaTime::try_from(1_234_567).unwrap(),
        MediaTime::try_from(7_777_777_777).unwrap(),
        minutes(200),
    )
    .unwrap();
    let parts = plan_parts(odd);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].start(), odd.start());
    assert_eq!(parts[2].end(), odd.end());
    for pair in parts.windows(2) {
        assert_eq!(pair[0].end(), pair[1].start());
    }
    assert!(
        parts
            .iter()
            .all(|p| p.duration().micros() <= Rules::PART_MICROS_MAX)
    );
}
#[test]
fn part_titles_number_the_parts_and_fit_the_title_limit() {
    assert_eq!(
        part_title("Le Petit Prince", 1, 3),
        "Le Petit Prince — partie 1/3"
    );
    let long = "Une très longue histoire ".repeat(10);
    let title = part_title(&long, 12, 12);
    assert_eq!(title.chars().count(), Rules::TITLE_MAX);
    assert!(title.ends_with("… — partie 12/12"), "{title}");
    assert!(Rules::title(&title).is_ok());
}
#[test]
fn part_titles_follow_the_client_wording() {
    assert_eq!(
        part_title_with(
            "{title} — part {part} of {parts}",
            "The Little Prince",
            2,
            3
        ),
        "The Little Prince — part 2 of 3"
    );
    let long = "A very long story ".repeat(10);
    let title = part_title_with("{title} — part {part} of {parts}", &long, 12, 12);
    assert_eq!(title.chars().count(), Rules::TITLE_MAX);
    assert!(title.ends_with("… — part 12 of 12"), "{title}");
    // A template that would lose the numbering is not used.
    assert_eq!(
        part_title_with("{title}", "Histoire", 1, 2),
        part_title("Histoire", 1, 2)
    );
}
#[test]
fn a_long_media_becomes_one_chapter_of_ordered_parts() {
    let media = MediaItem {
        id: MediaId::new(),
        title: "Histoire".into(),
        author: String::new(),
        tags: vec![],
        original: AssetId::new(),
        info: AudioInfo {
            duration: MediaTime::try_from(7_200_000_000).unwrap(),
            bytes: 1,
            codec: "mp3".into(),
            sample_rate: 44_100,
            channels: 2,
        },
        provenance: Provenance::LocalFile {
            name: "histoire.mp3".into(),
        },
        recipes: vec![],
        icon: None,
        chapters: vec![],
    };
    let parts = [RecipeId::new(), RecipeId::new()];
    let mut playlist = Playlist::new("Soir".into()).unwrap();
    playlist.add_media_parts(&media, &parts, None).unwrap();
    let chapter = &playlist.chapters[0];
    assert_eq!(chapter.title, "Histoire");
    let titles: Vec<_> = chapter.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Histoire — partie 1/2", "Histoire — partie 2/2"]);
    for (track, recipe) in chapter.tracks.iter().zip(parts) {
        assert!(matches!(track.media, TrackMedia::Local { recipe: r, .. } if r == recipe));
    }
    playlist.validate().unwrap();
}

#[test]
fn new_wire_keys_fit_yoto_and_long_keys_shorten_stably() {
    assert!(ChapterKey::fresh().as_str().len() <= WIRE_KEY_MAX);
    assert!(TrackKey::fresh().as_str().len() <= WIRE_KEY_MAX);
    let long = "0123456789abcdef0123456789abcdef";
    let mut p = Playlist::new("Chansons".into()).unwrap();
    p.chapters.push(Chapter {
        id: ChapterId::new(),
        key: ChapterKey::new(long).unwrap(),
        title: "Trois p'tits chats".into(),
        icon: None,
        extra: Extra::new(),
        tracks: vec![Track {
            id: TrackId::new(),
            key: TrackKey::new(long).unwrap(),
            title: "Trois p'tits chats".into(),
            icon: None,
            events: None,
            extra: Extra::new(),
            media: TrackMedia::Stream {
                url: "https://example.com/a.mp3".into(),
                format: AudioFormat::Mp3,
            },
        }],
    });
    assert!(matches!(p.validate(), Err(Failure::Validation(m)) if m == messages::WIRE_KEY_LENGTH));
    let before = p.clone();
    p.fit_wire_keys();
    p.validate().unwrap();
    assert_eq!(p.chapters[0].key.as_str(), &long[..WIRE_KEY_MAX]);
    assert_eq!(p.chapters[0].tracks[0].key.as_str(), &long[..WIRE_KEY_MAX]);
    let mut again = p.clone();
    again.fit_wire_keys();
    assert_eq!(again.chapters[0].key, p.chapters[0].key);
    assert_eq!(again.chapters[0].tracks[0].key, p.chapters[0].tracks[0].key);
    let mut remote = p.clone();
    remote.chapters[0].id = ChapterId::new();
    remote.chapters[0].tracks[0].id = TrackId::new();
    remote.retain_ids_from(&before);
    assert_eq!(remote.chapters[0].id, before.chapters[0].id);
    assert_eq!(
        remote.chapters[0].tracks[0].id,
        before.chapters[0].tracks[0].id
    );
}

#[test]
fn a_media_icon_goes_to_its_chapter_and_every_track() {
    let icon = AssetId::new();
    let media = MediaItem {
        id: MediaId::new(),
        title: "Histoire".into(),
        author: String::new(),
        tags: vec![],
        original: AssetId::new(),
        info: AudioInfo {
            duration: MediaTime::try_from(60_000_000).unwrap(),
            bytes: 1,
            codec: "mp3".into(),
            sample_rate: 44_100,
            channels: 2,
        },
        provenance: Provenance::LocalFile {
            name: "histoire.mp3".into(),
        },
        recipes: vec![],
        icon: Some(IconReference::Local { asset: icon }),
        chapters: vec![],
    };
    let local = |reference: &Option<IconReference>| match reference {
        Some(IconReference::Local { asset }) => Some(*asset),
        Some(IconReference::Yoto { .. }) | None => None,
    };
    let mut playlist = Playlist::new("Soir".into()).unwrap();
    playlist.add_media(&media, RecipeId::new()).unwrap();
    playlist
        .add_media_parts(&media, &[RecipeId::new(), RecipeId::new()], None)
        .unwrap();
    assert_eq!(playlist.chapters[1].tracks.len(), 2);
    for chapter in &playlist.chapters {
        assert_eq!(local(&chapter.icon), Some(icon));
        for track in &chapter.tracks {
            assert_eq!(local(&track.icon), Some(icon));
        }
    }
    assert_eq!(
        playlist.image_assets(),
        std::collections::BTreeSet::from([icon])
    );
    // Media stored before icons existed still load, without one.
    let mut stored = serde_json::to_value(&media).unwrap();
    stored.as_object_mut().unwrap().remove("icon");
    let old: MediaItem = serde_json::from_value(stored).unwrap();
    assert!(old.icon.is_none());
}
#[test]
fn playback_starts_where_asked_inside_the_range_and_never_too_close_to_its_end() {
    let at = |s: f64| MediaTime::from_seconds(s).unwrap();
    let range = TrimRange::new(at(10.), at(40.), at(60.)).unwrap();
    assert_eq!(
        range.played_from(at(25.)),
        TrimRange::new(at(25.), at(40.), at(60.)).unwrap()
    );
    assert_eq!(range.played_from(at(10.)), range);
    // Outside the range, playback starts at its beginning.
    assert_eq!(range.played_from(at(5.)), range);
    assert_eq!(range.played_from(at(40.)), range);
    assert_eq!(range.played_from(at(55.)), range);
    // In the last second, a whole second is still played.
    assert_eq!(range.played_from(at(39.8)).start(), at(39.));
    let short = TrimRange::new(at(10.), at(10.5), at(60.)).unwrap();
    assert_eq!(short.played_from(at(10.3)), short);
}

/// A playlist of stream tracks, one chapter per name holding the tracks listed after it.
fn structured(chapters: &[(&str, &[&str])]) -> Playlist {
    let mut p = Playlist::new("Structure".into()).unwrap();
    for (name, tracks) in chapters {
        p.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(*name).unwrap(),
            title: (*name).into(),
            icon: None,
            tracks: tracks
                .iter()
                .map(|track| Track {
                    id: TrackId::new(),
                    key: TrackKey::new(*track).unwrap(),
                    title: (*track).into(),
                    icon: None,
                    events: None,
                    media: TrackMedia::Stream {
                        url: "https://example.com/audio.mp3".into(),
                        format: AudioFormat::Mp3,
                    },
                    extra: Extra::new(),
                })
                .collect(),
            extra: Extra::new(),
        });
    }
    p
}
fn layout(p: &Playlist) -> Vec<(String, Vec<String>)> {
    p.chapters
        .iter()
        .map(|c| {
            (
                c.title.clone(),
                c.tracks.iter().map(|t| t.title.clone()).collect(),
            )
        })
        .collect()
}
fn local_icon(reference: &Option<IconReference>) -> Option<AssetId> {
    match reference {
        Some(IconReference::Local { asset }) => Some(*asset),
        Some(IconReference::Yoto { .. }) | None => None,
    }
}
fn media_fixture(title: &str, seconds: f64, icon: Option<AssetId>) -> MediaItem {
    MediaItem {
        id: MediaId::new(),
        title: title.into(),
        author: String::new(),
        tags: vec![],
        original: AssetId::new(),
        info: AudioInfo {
            duration: MediaTime::from_seconds(seconds).unwrap(),
            bytes: 1,
            codec: "mp3".into(),
            sample_rate: 44_100,
            channels: 2,
        },
        provenance: Provenance::LocalFile {
            name: format!("{title}.mp3"),
        },
        recipes: vec![],
        icon: icon.map(|asset| IconReference::Local { asset }),
        chapters: vec![],
    }
}
#[test]
fn tracks_leaving_a_chapter_keep_the_icon_they_showed() {
    let icon = AssetId::new();
    let own = AssetId::new();
    let mut p = structured(&[("Album", &["a", "b", "c"]), ("Solo", &["s"])]);
    p.chapters[0].icon = Some(IconReference::Local { asset: icon });
    p.chapters[0].tracks[1].icon = Some(IconReference::Local { asset: own });
    let album = p.chapters[0].id;
    let (a, b, c) = (
        p.chapters[0].tracks[0].id,
        p.chapters[0].tracks[1].id,
        p.chapters[0].tracks[2].id,
    );
    // Ungrouping: every new chapter and its track look as before.
    let mut ungrouped = p.clone();
    ungrouped.ungroup(album).unwrap();
    assert_eq!(ungrouped.chapters.len(), 4);
    // The new chapter carries the icon; its track goes on inheriting it.
    assert_eq!(local_icon(&ungrouped.chapters[0].icon), Some(icon));
    assert!(ungrouped.chapters[0].tracks[0].icon.is_none());
    assert_eq!(local_icon(&ungrouped.chapters[1].icon), Some(own));
    assert_eq!(local_icon(&ungrouped.chapters[1].tracks[0].icon), Some(own));
    ungrouped.validate().unwrap();
    // Moving into another chapter: the track brings its chapter's icon along.
    let solo = p.chapters[1].id;
    let mut moved = p.clone();
    moved.move_track(a, solo, 1).unwrap();
    assert_eq!(local_icon(&moved.chapters[1].tracks[1].icon), Some(icon));
    moved.move_track(b, solo, 0).unwrap();
    assert_eq!(local_icon(&moved.chapters[1].tracks[0].icon), Some(own));
    // Within its chapter, a track keeps relying on the chapter's icon.
    let mut within = p.clone();
    within.move_track(a, album, 2).unwrap();
    assert!(within.chapters[0].tracks[2].icon.is_none());
    // Extracting and splitting pass the icon on the same way.
    let mut extracted = p.clone();
    let new = extracted.extract_track(c, None).unwrap();
    assert_eq!(extracted.chapters[1].id, new);
    assert_eq!(local_icon(&extracted.chapters[1].icon), Some(icon));
    assert!(extracted.chapters[1].tracks[0].icon.is_none());
    let mut split = p.clone();
    split.split_chapter(b).unwrap();
    assert_eq!(local_icon(&split.chapters[1].icon), Some(icon));
    assert_eq!(local_icon(&split.chapters[1].tracks[0].icon), Some(own));
    assert_eq!(local_icon(&split.chapters[1].tracks[1].icon), Some(icon));
    // Merging the album into the solo chapter: the album's tracks keep its icon.
    let mut merged = p.clone();
    merged.merge_chapters(solo, album).unwrap();
    assert_eq!(merged.chapters.len(), 1);
    assert!(merged.chapters[0].icon.is_none());
    assert_eq!(local_icon(&merged.chapters[0].tracks[1].icon), Some(icon));
    assert_eq!(local_icon(&merged.chapters[0].tracks[2].icon), Some(own));
}
#[test]
fn chapters_are_moved_extracted_split_and_merged_in_place() {
    let p = structured(&[
        ("A", &["a1", "a2", "a3"]),
        ("B", &["b1"]),
        ("C", &["c1", "c2"]),
    ]);
    let (a, b, c) = (p.chapters[0].id, p.chapters[1].id, p.chapters[2].id);
    let (a2, a3, b1, c2) = (
        p.chapters[0].tracks[1].id,
        p.chapters[0].tracks[2].id,
        p.chapters[1].tracks[0].id,
        p.chapters[2].tracks[1].id,
    );
    let titles = |p: &Playlist| -> Vec<String> { layout(p).into_iter().map(|c| c.0).collect() };
    let mut moved = p.clone();
    moved.move_chapter_to(c, 0).unwrap();
    assert_eq!(titles(&moved), ["C", "A", "B"]);
    moved.move_chapter_to(c, 2).unwrap();
    assert_eq!(titles(&moved), ["A", "B", "C"]);
    assert!(moved.move_chapter_to(c, 3).is_err());
    let revision = moved.revision;
    moved.move_chapter_to(c, 2).unwrap();
    assert_eq!(moved.revision, revision, "no move, no change");
    // A track extracted lands right after its chapter, or where asked.
    let mut extracted = p.clone();
    extracted.extract_track(a2, None).unwrap();
    assert_eq!(
        layout(&extracted),
        [
            ("A".into(), vec!["a1".into(), "a3".into()]),
            ("a2".into(), vec!["a2".into()]),
            ("B".into(), vec!["b1".into()]),
            ("C".into(), vec!["c1".into(), "c2".into()]),
        ]
    );
    extracted.validate().unwrap();
    let mut placed = p.clone();
    placed.extract_track(a3, Some(0)).unwrap();
    assert_eq!(titles(&placed), ["a3", "A", "B", "C"]);
    placed.extract_track(c2, Some(4)).unwrap();
    assert_eq!(titles(&placed), ["a3", "A", "B", "C", "c2"]);
    assert!(placed.extract_track(c2, Some(6)).is_err());
    // The only track of a chapter just moves that chapter.
    let mut lone = p.clone();
    assert_eq!(lone.extract_track(b1, Some(0)).unwrap(), b);
    assert_eq!(titles(&lone), ["B", "A", "C"]);
    assert_eq!(lone.chapters.len(), 3);
    // Splitting before a track; the first track has nothing before it.
    let mut split = p.clone();
    split.split_chapter(a3).unwrap();
    assert_eq!(
        layout(&split)[..2],
        [
            ("A".into(), vec!["a1".into(), "a2".into()]),
            ("a3".into(), vec!["a3".into()]),
        ]
    );
    assert!(matches!(
        split.split_chapter(p.chapters[0].tracks[0].id),
        Err(Failure::Validation(m)) if m == messages::TRACK_BOUNDARY
    ));
    // Merging appends and keeps the target's title.
    let mut merged = p.clone();
    merged.merge_chapters(c, a).unwrap();
    assert_eq!(
        layout(&merged),
        [
            ("B".into(), vec!["b1".into()]),
            (
                "C".into(),
                vec![
                    "c1".into(),
                    "c2".into(),
                    "a1".into(),
                    "a2".into(),
                    "a3".into()
                ]
            ),
        ]
    );
    merged.validate().unwrap();
    let mut backwards = p.clone();
    backwards.merge_chapters(a, c).unwrap();
    assert_eq!(titles(&backwards), ["A", "B"]);
    assert_eq!(backwards.chapters[0].tracks.len(), 5);
    // Colliding wire keys refuse the merge and leave the playlist as it was.
    let mut colliding = structured(&[("X", &["1"]), ("Y", &["1"])]);
    let before = serde_json::to_value(&colliding).unwrap();
    let (x, y) = (colliding.chapters[0].id, colliding.chapters[1].id);
    assert!(matches!(
        colliding.merge_chapters(x, y),
        Err(Failure::Compatibility(_))
    ));
    assert_eq!(serde_json::to_value(&colliding).unwrap(), before);
}
#[test]
fn structure_changes_follow_or_refuse_shuffle_ranges() {
    let mut p = structured(&[("A", &["a1", "a2"]), ("B", &["b1"]), ("C", &["c1"])]);
    p.playback.shuffle = vec![ShuffleGroup {
        start: 1,
        end: 2,
        limit: 2,
    }];
    let (a, b, c) = (p.chapters[0].id, p.chapters[1].id, p.chapters[2].id);
    // Moving the range's chapters together keeps the range on them.
    let mut moved = p.clone();
    moved.move_chapter_to(a, 2).unwrap();
    let range = &moved.playback.shuffle[0];
    assert_eq!((range.start, range.end, range.limit), (0, 1, 2));
    moved.validate().unwrap();
    // Merging a chapter of the range away shrinks the range, and empties it when alone.
    let mut merged = p.clone();
    merged.merge_chapters(a, b).unwrap();
    let range = &merged.playback.shuffle[0];
    assert_eq!((range.start, range.end, range.limit), (1, 1, 1));
    merged.merge_chapters(a, c).unwrap();
    assert!(merged.playback.shuffle.is_empty());
    merged.validate().unwrap();
    // A new chapter inside the range would change what is shuffled: refused, untouched.
    let before = serde_json::to_value(&p).unwrap();
    let a2 = p.chapters[0].tracks[1].id;
    assert!(matches!(
        p.extract_track(a2, Some(2)),
        Err(Failure::Compatibility(_))
    ));
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
    // Outside the range it is fine, and the range follows.
    p.extract_track(a2, Some(0)).unwrap();
    let range = &p.playback.shuffle[0];
    assert_eq!((range.start, range.end), (2, 3));
    p.validate().unwrap();
}
#[test]
fn media_tracks_join_a_named_chapter_or_make_their_own() {
    let icon = AssetId::new();
    let media = media_fixture("Comptine", 60., Some(icon));
    let mut p = structured(&[("Album", &["a1"])]);
    let album = p.chapters[0].id;
    let track = |title: &str| MediaTrack {
        recipe: RecipeId::new(),
        title: title.into(),
    };
    p.add_media_tracks(
        &media,
        "Chapitre",
        vec![track("Un"), track("Deux")],
        Some(album),
    )
    .unwrap();
    assert_eq!(
        layout(&p),
        [(
            "Album".into(),
            vec!["a1".into(), "Un".into(), "Deux".into()]
        )]
    );
    assert_eq!(local_icon(&p.chapters[0].tracks[2].icon), Some(icon));
    p.add_media_tracks(&media, "Chapitre", vec![track("Trois")], None)
        .unwrap();
    assert_eq!(layout(&p)[1], ("Chapitre".into(), vec!["Trois".into()]));
    assert_eq!(local_icon(&p.chapters[1].icon), Some(icon));
    assert!(matches!(
        p.add_media_tracks(&media, "Vide", vec![], None),
        Err(Failure::Validation(m)) if m == messages::CHAPTER_EMPTY
    ));
    assert!(matches!(
        p.add_media_tracks(&media, "Ailleurs", vec![track("Quatre")], Some(ChapterId::new())),
        Err(Failure::Validation(m)) if m == messages::DESTINATION_ABSENT
    ));
    p.validate().unwrap();
}
#[test]
fn chapter_marks_must_cover_the_audio_in_order() {
    let at = |s: f64| MediaTime::from_seconds(s).unwrap();
    let mark = |s: f64, title: &str| MediaChapter {
        start: at(s),
        title: title.into(),
    };
    let mut media = media_fixture("Histoire", 100., None);
    media
        .set_chapters(vec![mark(0., ""), mark(30., "Milieu"), mark(70., "")])
        .unwrap();
    assert_eq!(media.chapters.len(), 3);
    let invalid = |chapters: Vec<MediaChapter>| {
        matches!(
            media.clone().set_chapters(chapters),
            Err(Failure::Validation(m)) if m == messages::CHAPTER_MARKS_INVALID
        )
    };
    assert!(invalid(vec![mark(10., ""), mark(30., "")]));
    assert!(invalid(vec![mark(0., ""), mark(30., ""), mark(30., "")]));
    assert!(invalid(vec![mark(0., ""), mark(50., ""), mark(40., "")]));
    assert!(invalid(vec![mark(0., ""), mark(100., "")]));
    assert!(matches!(
        media.clone().set_chapters(vec![mark(0., ""), mark(1., &"x".repeat(141))]),
        Err(Failure::Validation(m)) if m == messages::TITLE_LENGTH
    ));
    // One chapter is no chapter at all; the media is stored without marks.
    media.set_chapters(vec![mark(0., "Tout")]).unwrap();
    assert!(media.chapters.is_empty());
    // Media stored before marks existed still load, without any.
    let mut stored = serde_json::to_value(&media).unwrap();
    stored.as_object_mut().unwrap().remove("chapters");
    let old: MediaItem = serde_json::from_value(stored).unwrap();
    assert!(old.chapters.is_empty());
}
#[test]
fn a_version_is_cut_along_the_marks_it_keeps() {
    let at = |s: f64| MediaTime::from_seconds(s).unwrap();
    let range = |a: f64, b: f64| TrimRange::new(at(a), at(b), at(100.)).unwrap();
    let mark = |s: f64, title: &str| MediaChapter {
        start: at(s),
        title: title.into(),
    };
    let marks = vec![mark(0., ""), mark(30., "Milieu"), mark(70., " Fin ")];
    // The whole audio: one chapter per mark, titled when the mark is.
    let whole = plan_chapters(range(0., 100.), &marks);
    assert_eq!(
        whole,
        vec![
            PlannedChapter {
                title: None,
                ranges: vec![range(0., 30.)]
            },
            PlannedChapter {
                title: Some("Milieu".into()),
                ranges: vec![range(30., 70.)]
            },
            PlannedChapter {
                title: Some("Fin".into()),
                ranges: vec![range(70., 100.)]
            },
        ]
    );
    // A trim keeps the parts of the chapters it overlaps, and nothing of the others.
    let trimmed = plan_chapters(range(40., 80.), &marks);
    assert_eq!(trimmed.len(), 2);
    assert_eq!(trimmed[0].ranges, vec![range(40., 70.)]);
    assert_eq!(trimmed[0].title.as_deref(), Some("Milieu"));
    assert_eq!(trimmed[1].ranges, vec![range(70., 80.)]);
    // A mark on the trim's edge starts no empty chapter.
    let edge = plan_chapters(range(30., 70.), &marks);
    assert_eq!(edge.len(), 1);
    assert_eq!(edge[0].ranges, vec![range(30., 70.)]);
    // Without marks, only the length cuts the audio.
    let plain = plan_chapters(range(5., 95.), &[]);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].title, None);
    assert_eq!(plain[0].ranges, vec![range(5., 95.)]);
    // A long chapter is still cut for Yoto's track limit.
    let hours = TrimRange::new(MediaTime::ZERO, at(7200.), at(7200.)).unwrap();
    let long = plan_chapters(hours, &[mark(0., "Un"), mark(600., "Deux")]);
    assert_eq!(long[0].ranges.len(), 1);
    assert_eq!(long[1].ranges.len(), 2);
    assert_eq!(long[1].ranges[0].start(), at(600.));
    assert_eq!(long[1].ranges[1].end(), at(7200.));
}
