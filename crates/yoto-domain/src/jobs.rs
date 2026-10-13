use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum JobRequest {
    Import {
        selection: ImportSelection,
    },
    Encode {
        recipe: RecipeId,
    },
    Publish {
        account: AccountId,
        snapshot: Box<Playlist>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum JobState {
    Queued,
    Running { phase: JobPhase },
    Retry { at: u64, failure: Failure },
    WaitingForAuthentication,
    Conflict { remote: serde_json::Value },
    Reconciling,
    Completed { output: JobOutput },
    Failed { failure: Failure },
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum JobPhase {
    Acquiring,
    Analyzing,
    Encoding,
    Uploading {
        recipe: RecipeId,
        upload: Option<UploadId>,
    },
    Ingesting {
        recipe: RecipeId,
        upload: UploadId,
    },
    Committing,
    Verifying {
        card: CardId,
    },
}
impl JobPhase {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Acquiring => "Acquisition",
            Self::Analyzing => "Analyse",
            Self::Encoding => "Encodage local",
            Self::Uploading { .. } => "Envoi audio",
            Self::Ingesting { .. } => "Vérification Yoto",
            Self::Committing => "Publication",
            Self::Verifying { .. } => "Relecture",
        }
    }
}
impl JobState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "En attente",
            Self::Running { phase } => phase.label(),
            Self::Retry { .. } => "Nouvel essai programmé",
            Self::WaitingForAuthentication => "Connexion requise",
            Self::Conflict { .. } => "Conflit",
            Self::Reconciling => "Résultat distant à vérifier",
            Self::Completed { .. } => "Terminé",
            Self::Failed { .. } => "Échec",
            Self::Cancelled => "Annulé",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum JobOutput {
    Imported { media: MediaId },
    Encoded { asset: AssetId },
    Published { card: CardId, revision: Revision },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    pub id: JobId,
    pub request: JobRequest,
    pub state: JobState,
    pub attempts: u32,
    pub created: u64,
    pub checkpoint: Option<JobPhase>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublicationMapping {
    pub account: AccountId,
    pub recipe: RecipeId,
    pub digest: LocalSha256,
    pub media: YotoMediaId,
    pub info: AudioInfo,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublicationBinding {
    pub account: AccountId,
    pub playlist: PlaylistId,
    pub card: CardId,
    pub revision: Revision,
    pub document: serde_json::Value,
}
/// Durable intent for the remote-authoritative UI. Proposed changes live in the
/// publication journal; the confirmed catalogue is changed only with its receipt.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum CloudPlaylistWrite {
    Create,
    Update { expected: Revision },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Account {
    pub id: AccountId,
    pub label: String,
    pub scopes: Vec<Scope>,
}

/// Independently refreshed Yoto collections, scoped to the active account.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum RemoteCollection {
    Playlists,
    Devices,
    Groups,
    Icons,
}
impl RemoteCollection {
    pub const ALL: &[Self] = &[Self::Playlists, Self::Devices, Self::Groups, Self::Icons];
    pub fn label(self) -> &'static str {
        match self {
            Self::Playlists => "Playlists Yoto",
            Self::Devices => "Lecteurs",
            Self::Groups => "Groupes",
            Self::Icons => "Icônes",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum RemoteSyncState {
    Syncing,
    Ready { synced: u64 },
    Failed { failure: Failure },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum MediaSyncState {
    LocalOnly,
    Uploading { job: JobId },
    Uploaded { media: YotoMediaId },
    Verify { job: JobId },
    Failed { job: JobId, failure: Failure },
}
impl MediaSyncState {
    /// Index the latest publication once per recipe, then prefer confirmed media mappings.
    pub fn project_all(
        account: &AccountId,
        mappings: &[PublicationMapping],
        jobs: &[Job],
    ) -> std::collections::BTreeMap<RecipeId, Self> {
        let mut states = std::collections::BTreeMap::new();
        for job in jobs {
            if let JobRequest::Publish {
                account: a,
                snapshot,
            } = &job.request
            {
                if a != account {
                    continue;
                }
                let state = match &job.state {
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication => Self::Uploading { job: job.id },
                    JobState::Reconciling | JobState::Conflict { .. } => {
                        Self::Verify { job: job.id }
                    }
                    JobState::Failed { failure } => Self::Failed {
                        job: job.id,
                        failure: failure.clone(),
                    },
                    JobState::Completed { .. } | JobState::Cancelled => Self::LocalOnly,
                };
                for track in snapshot.chapters.iter().flat_map(|c| &c.tracks) {
                    if let TrackMedia::Local { recipe, .. } = &track.media {
                        states.insert(*recipe, state.clone());
                    }
                }
            }
        }
        for mapping in mappings.iter().filter(|m| &m.account == account) {
            states.insert(
                mapping.recipe,
                Self::Uploaded {
                    media: mapping.media.clone(),
                },
            );
        }
        states
    }
    pub fn project(
        account: &AccountId,
        recipe: RecipeId,
        mappings: &[PublicationMapping],
        jobs: &[Job],
    ) -> Self {
        if let Some(m) = mappings
            .iter()
            .find(|m| &m.account == account && m.recipe == recipe)
        {
            return Self::Uploaded {
                media: m.media.clone(),
            };
        }
        if let Some(job) =
            jobs.iter().rev().find(|j| match &j.request {
                JobRequest::Publish {
                    account: a,
                    snapshot,
                } => {
                    a == account
                        && snapshot.chapters.iter().flat_map(|c| &c.tracks).any(
                            |t| matches!(&t.media,TrackMedia::Local{recipe:r,..} if *r==recipe),
                        )
                }
                JobRequest::Import { .. } | JobRequest::Encode { .. } => false,
            })
        {
            return match &job.state {
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication => Self::Uploading { job: job.id },
                JobState::Reconciling | JobState::Conflict { .. } => Self::Verify { job: job.id },
                JobState::Failed { failure } => Self::Failed {
                    job: job.id,
                    failure: failure.clone(),
                },
                JobState::Completed { .. } | JobState::Cancelled => Self::LocalOnly,
            };
        }
        Self::LocalOnly
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::LocalOnly => "Local uniquement",
            Self::Uploading { .. } => "Envoi en cours",
            Self::Uploaded { .. } => "Envoyé sur Yoto",
            Self::Verify { .. } => "À vérifier",
            Self::Failed { .. } => "Échec d’envoi",
        }
    }
}

/// Durable remote effect, independent of the job's presentation state and last phase.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum PublicationOutcome {
    Prepared,
    Possible,
    Accepted { card: CardId },
    Confirmed { card: CardId, revision: Revision },
    Rejected,
}
impl PublicationOutcome {
    pub fn uncertain(&self) -> bool {
        match self {
            Self::Possible | Self::Accepted { .. } => true,
            Self::Prepared | Self::Confirmed { .. } | Self::Rejected => false,
        }
    }
    pub fn card(&self) -> Option<&CardId> {
        match self {
            Self::Accepted { card } | Self::Confirmed { card, .. } => Some(card),
            Self::Prepared | Self::Possible | Self::Rejected => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PendingPublication {
    pub job: JobId,
    pub account: AccountId,
    pub document: serde_json::Value,
    pub outcome: PublicationOutcome,
}
impl<'de> Deserialize<'de> for PendingPublication {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored {
            job: JobId,
            account: AccountId,
            document: serde_json::Value,
            #[serde(default)]
            outcome: Option<PublicationOutcome>,
            #[serde(default)]
            card: Option<CardId>,
        }
        let stored = Stored::deserialize(deserializer)?;
        // Older receipts had only a card/phase. Unproven effects remain conservative.
        let outcome = stored.outcome.unwrap_or(match stored.card {
            Some(card) => PublicationOutcome::Accepted { card },
            None => PublicationOutcome::Possible,
        });
        Ok(Self {
            job: stored.job,
            account: stored.account,
            document: stored.document,
            outcome,
        })
    }
}
