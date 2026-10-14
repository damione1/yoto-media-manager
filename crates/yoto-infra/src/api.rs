use crate::storage;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
use yoto_api_generated::apis::{
    self, configuration::Configuration, content_api, devices_api, family_api,
    family_library_groups_api, icons_api, media_api,
};
use yoto_domain::*;
#[derive(Clone)]
pub struct YotoGateway {
    http: reqwest::Client,
    pub host: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteCard {
    pub card: CardId,
    pub title: String,
    pub document: Value,
    pub synced: u64,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Icon {
    pub media: YotoMediaId,
    pub title: String,
    pub url: Option<String>,
    pub collection: IconCollection,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IconCollection {
    Public,
    User,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub image: Option<FamilyImageId>,
    pub cards: Vec<CardId>,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UploadReservation {
    #[serde(rename = "uploadId")]
    pub id: UploadId,
    #[serde(rename = "uploadUrl")]
    pub url: Option<String>,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Ingested {
    #[serde(rename = "transcodedSha256")]
    pub media: YotoMediaId,
    #[serde(rename = "transcodedInfo")]
    pub info: IngestedInfo,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct IngestedInfo {
    pub duration: f64,
    #[serde(rename = "fileSize")]
    pub bytes: u64,
    pub channels: AudioChannels,
    pub format: AudioFormat,
}
impl Ingested {
    pub fn audio_info(&self) -> Result<AudioInfo> {
        let channels = match self.info.channels {
            AudioChannels::Mono => 1,
            AudioChannels::Stereo => 2,
        };
        Ok(AudioInfo {
            duration: MediaTime::from_seconds(self.info.duration)?,
            bytes: self.info.bytes,
            codec: self.info.format.wire().into(),
            sample_rate: Rules::SAMPLE_RATE,
            channels,
        })
    }
}
/// Time allowed to open a connection, for API calls and file uploads alike.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Slowest upstream throughput an upload is expected to sustain (about 400 kbit/s).
const UPLOAD_SLOWEST_BYTES_PER_SECOND: u64 = 50_000;
const UPLOAD_TIMEOUT_MIN: Duration = Duration::from_secs(5 * 60);
const UPLOAD_TIMEOUT_MAX: Duration = Duration::from_secs(45 * 60);
/// Whole-request limit for sending a file of `bytes` to signed storage.
///
/// The shared 30-second API limit covers the whole request, which a long story on a slow
/// connection exceeds; the upload instead gets time in proportion to its size.
pub fn upload_timeout(bytes: u64) -> Duration {
    Duration::from_secs(bytes / UPLOAD_SLOWEST_BYTES_PER_SECOND)
        .clamp(UPLOAD_TIMEOUT_MIN, UPLOAD_TIMEOUT_MAX)
}
impl YotoGateway {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(storage)?,
            host: "https://api.yotoplay.com".into(),
        })
    }
    pub fn configuration(&self, token: &str) -> Configuration {
        Configuration {
            base_path: self.host.clone(),
            client: self.http.clone(),
            bearer_access_token: Some(token.into()),
            ..Configuration::default()
        }
    }
    pub async fn library(&self, token: &str) -> Result<Vec<RemoteCard>> {
        let r = content_api::get_user_s_myo_content(&self.configuration(token), Some("false"))
            .await
            .map_err(classify)?;
        r.cards
            .into_iter()
            .map(|c| {
                let document = serde_json::to_value(&c).map_err(storage)?;
                Ok(RemoteCard {
                    card: CardId::new(c.card_id)?,
                    title: c.title,
                    document,
                    synced: crate::now(),
                })
            })
            .collect()
    }
    pub async fn card(&self, token: &str, id: &CardId) -> Result<Value> {
        self.card_if_present(token, id)
            .await?
            .ok_or_else(|| Failure::Validation(messages::YOTO_NOT_FOUND.into()))
    }
    pub async fn card_if_present(&self, token: &str, id: &CardId) -> Result<Option<Value>> {
        match content_api::get_content(&self.configuration(token), id.as_str(), None, None, None)
            .await
        {
            Ok(response) => serde_json::to_value(response.card)
                .map(Some)
                .map_err(storage),
            Err(apis::Error::ResponseError(response)) if response.status.as_u16() == 404 => {
                Ok(None)
            }
            Err(error) => Err(classify(error)),
        }
    }
    pub async fn delete_card(&self, token: &str, id: &CardId) -> Result<()> {
        content_api::delete_content(&self.configuration(token), id.as_str())
            .await
            .map_err(classify)?;
        Ok(())
    }
    pub fn prepare_card(document: &Value) -> Result<yoto_api_generated::models::Card> {
        serde_json::from_value(document.clone())
            .map_err(|e| Failure::Validation(format!("Invalid card document: {e}")))
    }
    pub async fn write_card(&self, token: &str, document: &Value) -> Result<CardId> {
        self.write_prepared_card(token, Self::prepare_card(document)?)
            .await
    }
    pub async fn write_prepared_card(
        &self,
        token: &str,
        map: yoto_api_generated::models::Card,
    ) -> Result<CardId> {
        let r = content_api::create_or_update_content(&self.configuration(token), Some(map))
            .await
            .map_err(|e| classify(e).uncertain_write())?;
        CardId::new(r.card.card_id).map_err(|_| Failure::Ambiguous)
    }
    pub async fn reserve(
        &self,
        token: &str,
        digest: &str,
        filename: &str,
    ) -> Result<UploadReservation> {
        let bytes = hex::decode(digest).map_err(storage)?;
        if bytes.len() != 32 {
            return Err(Failure::Validation("Invalid local digest".into()));
        }
        let hash = URL_SAFE_NO_PAD.encode(bytes);
        let r =
            media_api::get_an_upload_url(&self.configuration(token), Some(&hash), Some(filename))
                .await
                .map_err(classify)?;
        Ok(UploadReservation {
            id: UploadId::new(r.upload.upload_id)?,
            url: r.upload.upload_url.flatten(),
        })
    }
    pub async fn put_signed(&self, url: &str, path: &Path, bytes: u64) -> Result<()> {
        let url = url::Url::parse(url)
            .map_err(|_| Failure::Compatibility("Invalid signed storage URL".into()))?;
        if url.scheme() != "https" && !cfg!(test) {
            return Err(Failure::Compatibility("Storage URL must use HTTPS".into()));
        }
        let file = tokio::fs::File::open(path).await.map_err(storage)?;
        let stream = tokio_util::io::ReaderStream::new(file);
        let response = self
            .http
            .put(url)
            .timeout(upload_timeout(bytes))
            .header(reqwest::header::CONTENT_TYPE, "audio/mpeg")
            .header(reqwest::header::CONTENT_LENGTH, bytes)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await
            .map_err(|_| Failure::Connection)?;
        if !response.status().is_success() {
            return Err(Failure::Connection);
        }
        Ok(())
    }
    pub async fn ingestion(&self, token: &str, id: &UploadId) -> Result<Option<Ingested>> {
        let r = media_api::get_transcoded_upload(&self.configuration(token), id.as_str(), false)
            .await
            .map_err(classify)?;
        let r = serde_json::to_value(r).map_err(storage)?;
        let value = r
            .get("transcode")
            .ok_or_else(|| Failure::Compatibility("Transcode envelope missing".into()))?;
        match value.get("transcodedSha256") {
            Some(Value::String(_)) => serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|e| Failure::Compatibility(format!("Transcoded media fields: {e}"))),
            Some(Value::Null) | None => Ok(None),
            Some(_) => Err(Failure::Compatibility("Invalid transcodedSha256".into())),
        }
    }
    pub async fn devices(&self, token: &str) -> Result<Vec<Device>> {
        let r = devices_api::get_devices(&self.configuration(token))
            .await
            .map_err(classify)?;
        r.devices
            .into_iter()
            .map(|d| {
                let extra = d.extra.clone();
                Ok(Device {
                    id: DeviceId::new(d.device_id)?,
                    name: d.name,
                    online: d.online,
                    firmware: extra
                        .get("fwVersion")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    product: d.device_type,
                    extra,
                })
            })
            .collect()
    }
    pub async fn device_config(
        &self,
        token: &str,
        id: &DeviceId,
    ) -> Result<wire::DeviceConfigResponse> {
        let r = devices_api::get_device_config(&self.configuration(token), id.as_str())
            .await
            .map_err(classify)?;
        Ok(r)
    }
    pub async fn device_status(&self, token: &str, id: &DeviceId) -> Result<Value> {
        let r = devices_api::get_device_status(&self.configuration(token), id.as_str())
            .await
            .map_err(classify)?;
        Ok(json!(r))
    }
    pub async fn update_device(
        &self,
        token: &str,
        id: &DeviceId,
        config: wire::DeviceConfigWrite,
    ) -> Result<()> {
        devices_api::update_device_config(&self.configuration(token), id.as_str(), Some(config))
            .await
            .map_err(classify)?;
        Ok(())
    }
    pub async fn shortcuts(
        &self,
        token: &str,
        id: &DeviceId,
        config: yoto_api_generated::models::UpdateShortcutsBetaRequest,
    ) -> Result<()> {
        devices_api::update_shortcuts_beta(&self.configuration(token), id.as_str(), Some(config))
            .await
            .map_err(classify)
    }
    pub async fn icons(&self, token: &str) -> Result<Vec<Icon>> {
        let config = self.configuration(token);
        let public = icons_api::get_public_icons(&config)
            .await
            .map_err(classify)?;
        let own = icons_api::get_user_icons(&config).await.map_err(classify)?;
        let mut icons = vec![];
        for (collection, response) in [
            (IconCollection::Public, public),
            (IconCollection::User, own),
        ] {
            for i in response
                .display_icons
                .ok_or_else(|| Failure::Compatibility("Icons missing".into()))?
            {
                icons.push(Icon {
                    media: YotoMediaId::new(
                        i.media_id
                            .ok_or_else(|| Failure::Compatibility("Icon mediaId missing".into()))?,
                    )?,
                    title: i.title.unwrap_or_else(|| "Icône".into()),
                    url: i.url,
                    collection,
                });
            }
        }
        Ok(icons)
    }
    pub async fn upload_icon(&self, token: &str, path: &Path) -> Result<YotoMediaId> {
        let r = icons_api::upload_custom_icon(
            &self.configuration(token),
            path.to_owned(),
            Some("false"),
            Some("icon.png"),
        )
        .await
        .map_err(classify)?;
        let icon = r
            .display_icon
            .ok_or_else(|| Failure::Compatibility("Icon reply missing".into()))?;
        YotoMediaId::new(
            icon.media_id
                .ok_or_else(|| Failure::Compatibility("Icon mediaId missing".into()))?,
        )
    }
    pub async fn upload_cover(&self, token: &str, path: &Path) -> Result<String> {
        let r = media_api::upload_cover_image(
            &self.configuration(token),
            path.to_owned(),
            None,
            Some("false"),
            None,
            Some("cover.png"),
        )
        .await
        .map_err(classify)?;
        r.cover_image
            .and_then(|c| c.media_url)
            .ok_or_else(|| Failure::Compatibility("Cover mediaUrl missing".into()))
    }
    pub async fn groups(&self, token: &str) -> Result<Vec<Group>> {
        let groups = family_library_groups_api::get_groups(&self.configuration(token))
            .await
            .map_err(classify)?;
        groups
            .into_iter()
            .map(|g| {
                Ok(Group {
                    id: GroupId::new(
                        g.id.ok_or_else(|| Failure::Compatibility("Group id missing".into()))?,
                    )?,
                    name: g.name.unwrap_or_default(),
                    image: g.image_id.map(FamilyImageId::new).transpose()?,
                    cards: if let Some(items) = g.items {
                        items
                            .into_iter()
                            .map(|item| {
                                CardId::new(item.content_id.ok_or_else(|| {
                                    Failure::Compatibility("Group item contentId missing".into())
                                })?)
                            })
                            .collect::<Result<_>>()?
                    } else {
                        g.cards
                            .unwrap_or_default()
                            .into_iter()
                            .map(|card| {
                                CardId::new(card.as_str().ok_or_else(|| {
                                    Failure::Compatibility("Unsupported group card".into())
                                })?)
                            })
                            .collect::<Result<_>>()?
                    },
                })
            })
            .collect()
    }
    pub async fn save_group(
        &self,
        token: &str,
        id: Option<&GroupId>,
        name: &str,
        cards: &[CardId],
        image: Option<&str>,
    ) -> Result<Value> {
        if name.is_empty() || name.chars().count() > 100 {
            return Err(Failure::Validation(messages::GROUP_NAME_LENGTH.into()));
        }
        let config = self.configuration(token);
        match id {
            Some(id) => {
                if image.is_none() {
                    let current = family_library_groups_api::get_a_group(&config, id.as_str())
                        .await
                        .map_err(classify)?;
                    if current.image_id.is_some() {
                        return Err(Failure::Compatibility("Removing a group's existing image is not documented by the Yoto contract".into()));
                    }
                }
                let mut dto = wire::UpdateAGroupRequest::new();
                dto.name = Some(name.into());
                dto.image_id = image.map(str::to_owned);
                dto.items = Some(
                    cards
                        .iter()
                        .map(|card| {
                            let mut item = wire::UpdateAGroupRequestItemsInner::new();
                            item.content_id = Some(card.to_string());
                            item
                        })
                        .collect(),
                );
                let r = family_library_groups_api::update_a_group(&config, id.as_str(), Some(dto))
                    .await
                    .map_err(classify)?;
                if r.id != id.as_str() {
                    return Err(Failure::Ambiguous);
                }
                serde_json::to_value(r).map_err(storage)
            }
            None => {
                let mut dto = wire::CreateAGroupRequest::new();
                dto.name = Some(name.into());
                dto.image_id = image.map(str::to_owned);
                dto.items = Some(
                    cards
                        .iter()
                        .map(|card| {
                            let mut item = wire::CreateAGroupRequestItemsInner::new();
                            item.content_id = Some(card.to_string());
                            item
                        })
                        .collect(),
                );
                let r = family_library_groups_api::create_a_group(&config, Some(dto))
                    .await
                    .map_err(|e| classify(e).uncertain_write())?;
                GroupId::new(r.id.clone()).map_err(|_| Failure::Ambiguous)?;
                serde_json::to_value(r).map_err(storage)
            }
        }
    }
    pub async fn group(&self, token: &str, id: &GroupId) -> Result<Value> {
        let r = family_library_groups_api::get_a_group(&self.configuration(token), id.as_str())
            .await
            .map_err(classify)?;
        Ok(json!(r))
    }
    pub async fn delete_group(&self, token: &str, id: &GroupId) -> Result<()> {
        family_library_groups_api::delete_a_group(&self.configuration(token), id.as_str())
            .await
            .map_err(classify)?;
        Ok(())
    }
    pub async fn family_images(&self, token: &str) -> Result<Vec<FamilyImageId>> {
        let r = family_api::get_family_images(&self.configuration(token), Some("500"))
            .await
            .map_err(classify)?;
        r.images
            .into_iter()
            .map(|i| FamilyImageId::new(i.image_id))
            .collect()
    }
    pub async fn upload_family_image(&self, token: &str, path: &Path) -> Result<FamilyImageId> {
        if std::fs::metadata(path).map_err(storage)?.len() > 8_000_000 {
            return Err(Failure::Validation(messages::FAMILY_IMAGE_TOO_LARGE.into()));
        }
        let r = family_api::upload_a_family_image(&self.configuration(token), path.to_owned())
            .await
            .map_err(classify)?;
        FamilyImageId::new(r.image_id)
    }
    pub async fn family_image(
        &self,
        token: &str,
        image: &FamilyImageId,
        size: FamilyImageSize,
    ) -> Result<Vec<u8>> {
        let (width, height) = size.dimensions();
        let location = family_api::get_a_family_image(
            &self.configuration(token),
            image.as_str(),
            width,
            height,
        )
        .await
        .map_err(classify)?;
        let url = url::Url::parse(&location)
            .map_err(|_| Failure::Compatibility("Invalid image location".into()))?;
        if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
            return Err(Failure::Compatibility(
                "Image location must be HTTPS".into(),
            ));
        }
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| Failure::Connection)?;
        if !response.status().is_success() {
            return Err(Failure::Connection);
        }
        if response.content_length().is_some_and(|n| n > 8_000_000) {
            return Err(Failure::Validation(messages::IMAGE_TOO_LARGE.into()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Connection)? {
            if bytes.len() + chunk.len() > 8_000_000 {
                return Err(Failure::Validation(messages::IMAGE_TOO_LARGE.into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
pub fn classify<T>(e: apis::Error<T>) -> Failure {
    match e {
        apis::Error::Reqwest(_) => Failure::Connection,
        apis::Error::Serde(e) => Failure::Compatibility(e.to_string()),
        apis::Error::Io(e) => storage(e),
        apis::Error::ResponseError(e) => match e.status.as_u16() {
            401 => Failure::Authentication,
            403 => Failure::Permission(messages::YOTO_DENIED.into()),
            409 => Failure::Conflict,
            429 => Failure::RateLimit {
                retry_after_seconds: 30,
            },
            400 | 404 | 422 => {
                Failure::Validation(format!("Yoto rejected the request ({})", e.status.as_u16()))
            }
            500..=599 => Failure::Connection,
            _ => Failure::Compatibility(format!("Unexpected HTTP status {}", e.status.as_u16())),
        },
    }
}
fn object(v: &Value) -> Result<Extra> {
    serde_json::from_value(v.clone()).map_err(|_| Failure::Compatibility("Expected object".into()))
}
fn take_extra(v: &Value, known: &[&str]) -> Result<Extra> {
    let mut extra = object(v)?;
    for k in known {
        extra.remove(*k);
    }
    Ok(extra)
}
fn unmanaged<T: serde::Serialize>(wire: &T, managed: &[&str]) -> Result<Extra> {
    take_extra(&serde_json::to_value(wire).map_err(storage)?, managed)
}
/// Restore extension fields through their generated DTO, then assign managed fields by type.
fn preserve<T: serde::Serialize + serde::de::DeserializeOwned>(
    base: T,
    extra: &Extra,
) -> Result<T> {
    let mut fields = extra.clone();
    let known = serde_json::to_value(base).map_err(storage)?;
    fields.extend(object(&known)?);
    serde_json::from_value(json!(fields)).map_err(|e| Failure::Compatibility(e.to_string()))
}
use yoto_api_generated::models as wire;
fn read_icon(
    display: Option<&wire::Display>,
    account: &AccountId,
) -> Result<Option<IconReference>> {
    let Some(icon) = display
        .and_then(|d| d.icon16x16.as_ref())
        .and_then(|s| s.as_ref())
    else {
        return Ok(None);
    };
    let id = icon
        .strip_prefix("yoto:#")
        .ok_or_else(|| Failure::Compatibility("Unsupported icon reference".into()))?;
    Ok(Some(IconReference::Yoto {
        account: account.clone(),
        media: YotoMediaId::new(id)?,
    }))
}
pub fn read_playlist(document: &Value, account: &AccountId) -> Result<Playlist> {
    let card: wire::Card = serde_json::from_value(document.clone())
        .map_err(|e| Failure::Compatibility(format!("Card cannot be edited safely: {e}")))?;
    let mut p = Playlist::new(card.title.clone())?;
    p.origin = DraftOrigin::Existing {
        base: RemoteBase {
            account: account.clone(),
            card: CardId::new(
                card.card_id
                    .as_ref()
                    .ok_or_else(|| Failure::Compatibility("cardId missing".into()))?,
            )?,
            document: document.clone(),
        },
    };
    p.extra = unmanaged(
        &card,
        &[
            "cardId",
            "title",
            "content",
            "metadata",
            "createdAt",
            "updatedAt",
            "_id",
            "userId",
        ],
    )?;
    p.content_extra = unmanaged(&card.content, &["chapters", "config", "playbackType"])?;
    if let Some(kind) = card.content.playback_type {
        p.playback.kind = kind;
    }
    if let Some(config) = &card.content.config {
        p.playback.extra = unmanaged(
            config,
            &["autoadvance", "resumeTimeout", "onlineOnly", "shuffle"],
        )?;
        if let Some(advance) = config.autoadvance {
            p.playback.autoadvance = advance;
        }
        if let Some(seconds) = config.resume_timeout {
            p.playback.resume_seconds = seconds
                .try_into()
                .map_err(|_| Failure::Compatibility("Negative resume timeout".into()))?;
        }
        if let Some(online) = config.online_only {
            p.playback.online_only = online;
        }
        p.playback.shuffle = config
            .shuffle
            .as_ref()
            .map(|v| {
                v.iter()
                    .map(|g| {
                        Ok(ShuffleGroup {
                            start: g.start.try_into().map_err(storage)?,
                            end: g.end.try_into().map_err(storage)?,
                            limit: g.limit.try_into().map_err(storage)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
    }
    if let Some(m) = &card.metadata {
        p.metadata.description = m.description.clone().unwrap_or_default();
        p.metadata.author = m.author.clone().unwrap_or_default();
        p.metadata.narrator = m.read_by.clone().unwrap_or_default();
        p.metadata.languages = m.languages.clone().unwrap_or_default();
        if let Some(category) = m.category {
            p.metadata.category = category;
        }
        let age = |n: f64| -> Result<u8> {
            if n.fract() != 0. || !(0.0..=255.0).contains(&n) {
                Err(Failure::Compatibility("Invalid age".into()))
            } else {
                Ok(n as u8)
            }
        };
        p.metadata.min_age = m.min_age.map(age).transpose()?;
        p.metadata.max_age = m.max_age.map(age).transpose()?;
        p.metadata.extra = unmanaged(
            m,
            &[
                "description",
                "author",
                "readBy",
                "languages",
                "category",
                "minAge",
                "maxAge",
            ],
        )?;
    }
    for c in &card.content.chapters {
        let mut chapter = Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(c.key.clone())?,
            title: c.title.clone(),
            icon: read_icon(c.display.as_deref(), account)?,
            tracks: vec![],
            extra: unmanaged(c, &["key", "title", "tracks"])?,
        };
        for t in &c.tracks {
            let media = match t.r#type {
                TrackType::Audio => {
                    let remote = t.track_url.strip_prefix("yoto:#").ok_or_else(|| {
                        Failure::Compatibility("Unsupported audio reference".into())
                    })?;
                    TrackMedia::RemoteAudio {
                        account: account.clone(),
                        media: YotoMediaId::new(remote)?,
                        info: AudioInfo {
                            duration: MediaTime::from_seconds(t.duration)?,
                            bytes: t.file_size.try_into().map_err(storage)?,
                            codec: t.format.wire().into(),
                            sample_rate: Rules::SAMPLE_RATE,
                            channels: match t.channels.unwrap_or(AudioChannels::Stereo) {
                                AudioChannels::Mono => 1,
                                AudioChannels::Stereo => 2,
                            },
                        },
                    }
                }
                TrackType::Stream => TrackMedia::Stream {
                    url: t.track_url.clone(),
                    format: t.format,
                },
            };
            chapter.tracks.push(Track {
                id: TrackId::new(),
                key: TrackKey::new(t.key.clone())?,
                title: t.title.clone(),
                icon: read_icon(t.display.as_deref(), account)?,
                media,
                events: t.events.as_deref().cloned(),
                extra: unmanaged(
                    t,
                    &[
                        "key", "title", "trackUrl", "duration", "fileSize", "format", "channels",
                        "type", "events",
                    ],
                )?,
            });
        }
        p.chapters.push(chapter);
    }
    p.validate_draft()?;
    Ok(p)
}
pub fn write_playlist(
    p: &Playlist,
    account: &AccountId,
    mappings: &[PublicationMapping],
    icons: &BTreeMap<AssetId, YotoMediaId>,
    cover: Option<&str>,
) -> Result<Value> {
    p.validate_remote_write()?;
    let mut chapters = vec![];
    let mut bytes = 0u64;
    let mut duration = 0u64;
    for c in &p.chapters {
        let mut tracks = vec![];
        for t in &c.tracks {
            let (url, info, kind, format) = match &t.media {
                TrackMedia::Local { recipe, .. } => {
                    let m = mappings
                        .iter()
                        .find(|m| &m.account == account && m.recipe == *recipe)
                        .ok_or_else(|| {
                            Failure::Validation(
                                "Prepare and upload this track before publication".into(),
                            )
                        })?;
                    (
                        format!("yoto:#{}", m.media),
                        Some(&m.info),
                        TrackType::Audio,
                        None,
                    )
                }
                TrackMedia::RemoteAudio {
                    account: owner,
                    media,
                    info,
                } => {
                    if owner != account {
                        return Err(Failure::Validation(
                            "Remote audio belongs to another account".into(),
                        ));
                    }
                    (format!("yoto:#{media}"), Some(info), TrackType::Audio, None)
                }
                TrackMedia::Stream { url, format } => {
                    if url::Url::parse(url)
                        .map_err(|_| Failure::Validation(messages::STREAM_URL_INVALID.into()))?
                        .scheme()
                        != "https"
                    {
                        return Err(Failure::Validation(messages::STREAM_HTTPS_ONLY.into()));
                    }
                    (url.clone(), None, TrackType::Stream, Some(*format))
                }
            };
            let (track_duration, track_bytes, format, channels) = if let Some(info) = info {
                if info.bytes > Rules::TRACK_BYTES_MAX
                    || info.duration.micros() > Rules::TRACK_MICROS_MAX
                {
                    return Err(Failure::Validation(messages::TRACK_EXCEEDS_LIMITS.into()));
                }
                bytes = bytes
                    .checked_add(info.bytes)
                    .ok_or_else(|| Failure::Validation("Size overflow".into()))?;
                duration = duration
                    .checked_add(info.duration.micros())
                    .ok_or_else(|| Failure::Validation("Duration overflow".into()))?;
                let format: AudioFormat = serde_json::from_value(json!(info.codec))
                    .map_err(|_| Failure::Compatibility("Unsupported codec".into()))?;
                let channels = match info.channels {
                    1 => AudioChannels::Mono,
                    2 => AudioChannels::Stereo,
                    _ => return Err(Failure::Validation("Mono or stereo required".into())),
                };
                (
                    info.duration.seconds(),
                    i64::try_from(info.bytes).map_err(storage)?,
                    format,
                    Some(channels),
                )
            } else {
                (
                    0.,
                    0,
                    format.ok_or_else(|| Failure::Compatibility("Stream format missing".into()))?,
                    None,
                )
            };
            let mut track = preserve(
                wire::Track::new(
                    track_duration,
                    track_bytes,
                    format,
                    t.key.to_string(),
                    t.title.clone(),
                    url,
                    kind,
                ),
                &t.extra,
            )?;
            track.channels = channels;
            if let Some(events) = &t.events {
                track.events = Some(Box::new(events.clone()));
            }
            apply_icon(&mut track.display, &t.icon, account, icons)?;
            tracks.push(track);
        }
        let mut chapter = preserve(
            wire::Chapter::new(c.key.to_string(), c.title.clone(), tracks),
            &c.extra,
        )?;
        apply_icon(&mut chapter.display, &c.icon, account, icons)?;
        chapters.push(chapter);
    }
    if bytes > Rules::CARD_BYTES_MAX || duration > Rules::CARD_MICROS_MAX {
        return Err(Failure::Validation(
            messages::PLAYLIST_EXCEEDS_LIMITS.into(),
        ));
    }
    let mut config = preserve(wire::PlaybackConfig::new(), &p.playback.extra)?;
    config.autoadvance = Some(p.playback.autoadvance);
    config.resume_timeout = Some(p.playback.resume_seconds.try_into().map_err(storage)?);
    config.online_only = Some(p.playback.online_only);
    config.shuffle = Some(
        p.playback
            .shuffle
            .iter()
            .map(|g| {
                Ok(wire::ShuffleGroup::new(
                    g.end.try_into().map_err(storage)?,
                    g.limit.try_into().map_err(storage)?,
                    g.start.try_into().map_err(storage)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    );
    let mut content = preserve(wire::Content::new(chapters), &p.content_extra)?;
    content.config = Some(Box::new(config));
    content.playback_type = Some(p.playback.kind);
    let mut metadata = preserve(wire::Metadata::new(), &p.metadata.extra)?;
    metadata.description = Some(p.metadata.description.clone());
    metadata.author = Some(p.metadata.author.clone());
    metadata.read_by = Some(p.metadata.narrator.clone());
    metadata.languages = Some(p.metadata.languages.clone());
    metadata.category = Some(p.metadata.category);
    metadata.min_age = p.metadata.min_age.map(f64::from);
    metadata.max_age = p.metadata.max_age.map(f64::from);
    if let Some(url) = cover {
        let mut image = metadata
            .cover
            .take()
            .unwrap_or_else(|| Box::new(wire::MetadataCover::new()));
        image.image_l = Some(url.into());
        metadata.cover = Some(image);
    }
    let mut total = metadata
        .media
        .take()
        .unwrap_or_else(|| Box::new(wire::MetadataMedia::new()));
    total.duration = Some(duration as f64 / 1_000_000.);
    total.file_size = Some(bytes.try_into().map_err(storage)?);
    metadata.media = Some(total);
    let mut card = preserve(wire::Card::new(content, p.title.clone()), &p.extra)?;
    card.metadata = Some(Box::new(metadata));
    match &p.origin {
        DraftOrigin::New => card.card_id = None,
        DraftOrigin::Existing { base } => {
            if &base.account != account {
                return Err(Failure::Validation(
                    "Create a local copy before publishing to a different account".into(),
                ));
            }
            card.card_id = Some(base.card.to_string());
        }
    }
    serde_json::to_value(card).map_err(storage)
}
fn apply_icon(
    display: &mut Option<Box<wire::Display>>,
    icon: &Option<IconReference>,
    account: &AccountId,
    icons: &BTreeMap<AssetId, YotoMediaId>,
) -> Result<()> {
    let media = match icon {
        Some(IconReference::Local { asset }) => Some(
            icons
                .get(asset)
                .ok_or_else(|| Failure::Validation(messages::ICON_NOT_UPLOADED.into()))?,
        ),
        Some(IconReference::Yoto {
            account: owner,
            media,
        }) => {
            if owner != account {
                return Err(Failure::Validation(
                    "Icon belongs to another account".into(),
                ));
            }
            Some(media)
        }
        None => None,
    };
    if media.is_some() || display.is_some() {
        let d = display.get_or_insert_with(|| Box::new(wire::Display::new()));
        d.icon16x16 = media.map(|m| Some(format!("yoto:#{m}")));
    }
    Ok(())
}

pub fn patch_shortcuts(
    old: Option<&wire::DeviceRecordShortcuts>,
    day: Option<Vec<Shortcut>>,
    night: Option<Vec<Shortcut>>,
) -> Result<wire::UpdateShortcutsBetaRequest> {
    let mut shortcuts: wire::UpdateShortcutsBetaRequestShortcuts = match old {
        Some(old) => {
            serde_json::from_value(serde_json::to_value(old).map_err(storage)?).map_err(storage)?
        }
        None => wire::UpdateShortcutsBetaRequestShortcuts::new(),
    };
    let modes = shortcuts
        .modes
        .get_or_insert_with(|| Box::new(wire::UpdateShortcutsBetaRequestShortcutsModes::new()));
    let items=|items:Vec<Shortcut>| -> Result<Vec<wire::UpdateShortcutsBetaRequestShortcutsModesDayContentInner>> {
        items.into_iter().map(|item| {
            let mut params=wire::UpdateShortcutsBetaRequestShortcutsModesDayContentInnerParams::new();
            params.card=Some(item.card.to_string()); params.chapter=Some(item.chapter.to_string()); params.track=Some(item.track.to_string());
            let mut value=wire::UpdateShortcutsBetaRequestShortcutsModesDayContentInner::new(); value.cmd=Some("track-play".into()); value.params=Some(Box::new(params)); Ok(value)
        }).collect()
    };
    if let Some(day) = day {
        modes
            .day
            .get_or_insert_with(|| {
                Box::new(wire::UpdateShortcutsBetaRequestShortcutsModesDay::new())
            })
            .content = Some(items(day)?);
    }
    if let Some(night) = night {
        modes
            .night
            .get_or_insert_with(|| {
                Box::new(wire::UpdateShortcutsBetaRequestShortcutsModesNight::new())
            })
            .content = Some(
            items(night)?
                .into_iter()
                .map(|v| serde_json::to_value(v).map_err(storage))
                .collect::<Result<_>>()?,
        );
    }
    let mut request = wire::UpdateShortcutsBetaRequest::new();
    request.shortcuts = Some(Box::new(shortcuts));
    Ok(request)
}

/// Merge the complete typed settings patch onto freshly read server options.
pub fn patch_device_config(
    mut config: wire::DeviceSettings,
    patch: &DeviceConfiguration,
) -> Result<wire::DeviceConfigWrite> {
    patch.validate()?;
    config.day_time = Some(patch.day_time.clone());
    config.night_time = Some(patch.night_time.clone());
    config.max_volume_limit = Some(patch.max_volume_limit.clone());
    config.night_max_volume_limit = Some(patch.night_max_volume_limit.clone());
    config.ambient_colour = Some(patch.ambient_colour.clone());
    config.night_ambient_colour = Some(patch.night_ambient_colour.clone());
    config.display_dim_timeout = Some(patch.display_dim_timeout.clone());
    config.shutdown_timeout = Some(patch.shutdown_timeout.clone());
    config.repeat_all = Some(patch.repeat_all);
    config.bt_headphones_enabled = Some(patch.bt_headphones_enabled);
    config.headphones_volume_limited = Some(patch.headphones_volume_limited);
    if let Some(value) = &patch.clock_face {
        config.clock_face = Some(value.clone());
    }
    if let Some(value) = &patch.locale {
        config.locale = Some(value.clone());
    }
    if let Some(value) = &patch.hour_format {
        config.hour_format = Some(value.clone());
    }
    if let Some(value) = &patch.day_display_brightness {
        config.day_display_brightness = Some(value.clone());
    }
    if let Some(value) = &patch.night_display_brightness {
        config.night_display_brightness = Some(value.clone());
    }
    if let Some(value) = &patch.display_dim_brightness {
        config.display_dim_brightness = Some(value.clone());
    }
    if let Some(value) = &patch.day_yoto_daily {
        config.day_yoto_daily = Some(value.clone());
    }
    if let Some(value) = &patch.night_yoto_daily {
        config.night_yoto_daily = Some(value.clone());
    }
    if let Some(value) = &patch.day_yoto_radio {
        config.day_yoto_radio = Some(value.clone());
    }
    if let Some(value) = &patch.night_yoto_radio {
        config.night_yoto_radio = Some(value.clone());
    }
    if let Some(value) = &patch.volume_level {
        config.volume_level = Some(value.clone());
    }
    if let Some(value) = &patch.bluetooth_enabled {
        config.bluetooth_enabled = Some(value.clone());
    }
    Ok(wire::DeviceConfigWrite::new(config, patch.name.clone()))
}
