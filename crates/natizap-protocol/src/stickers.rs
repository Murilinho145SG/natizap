//! The account's favorite stickers, kept in WhatsApp's app state.
//!
//! whatsapp-rust syncs app state but drops what it has no event for, and
//! favorite stickers are among those; so natizap reads that collection
//! itself, with the same public pieces whatsapp-rust uses: the server's
//! `w:sync:app:state` answer, the app state keys in its store, and
//! `wacore-appstate` to decode. Reading leaves whatsapp-rust's own state
//! of the collection untouched.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use whatsapp_rust::Client;
use whatsapp_rust::NodeBuilder;
use whatsapp_rust::buffa::Message as _;
use whatsapp_rust::jid_utils::server_jid;
use whatsapp_rust::request::{InfoQuery, InfoQueryType};
use whatsapp_rust::wacore::appstate::hash::HashState;
use whatsapp_rust::wacore::appstate::patch_decode::{PatchList, parse_patch_list_ref};
use whatsapp_rust::wacore::appstate::{
    AppStateError, ExpandedAppStateKeys, Mutation, expand_app_state_keys, process_patch,
    process_snapshot, schemas,
};
use whatsapp_rust::wacore_binary::NodeContent;
use whatsapp_rust::waproto::whatsapp as wa;

use crate::actions::now_ms;
use crate::outgoing::media_message;
use crate::{ActionError, ContentKind, Media};

/// The app state collection favorite stickers live in.
const COLLECTION: &str = "regular_low";
/// The first part of a favorite sticker's index.
const FAVORITE: &str = "favoriteSticker";
/// Pages of patches read at most, against a server that never says it is
/// done.
const MOST_PAGES: usize = 50;

/// A sticker the account keeps among its favorites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sticker {
    /// Its identity: the hash of its file, in base64, as WhatsApp names it.
    pub hash: String,
    /// Its file on WhatsApp's servers.
    pub media: Media,
    /// A Lottie animation rather than a WebP image.
    pub lottie: bool,
}

impl Sticker {
    /// The sticker a message carries as `media`, to keep among favorites.
    #[must_use]
    pub fn of(media: Media) -> Self {
        Self {
            hash: BASE64.encode(&media.file_sha256),
            lottie: media.mime.as_deref() == Some("application/x-lottie"),
            media,
        }
    }
}

/// The account's favorite stickers, newest last, as its other devices
/// keep them.
pub(crate) async fn favorites(client: Arc<Client>) -> Result<Vec<Sticker>, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    let mut state = HashState::default();
    let mut mutations = Vec::new();
    let mut version = None;
    for _ in 0..MOST_PAGES {
        let mut list = fetch_page(&client, version).await?;
        inline_blobs(&client, &mut list).await?;
        let keys = keys_for(&client, &list).await?;
        mutations.extend(decode_page(&list, &keys, &mut state).map_err(|error| rejected(&error))?);
        if !list.has_more_patches {
            break;
        }
        version = Some(state.version);
    }
    Ok(favorites_in(&mutations))
}

/// What a page, its blobs in place, changes: its snapshot's records and
/// its patches' mutations, decrypted with `keys`, advancing `state`. MACs
/// are not checked: the page is only read, never built on.
fn decode_page(
    list: &PatchList,
    keys: &HashMap<Vec<u8>, Arc<ExpandedAppStateKeys>>,
    state: &mut HashState,
) -> Result<Vec<Mutation>, AppStateError> {
    let get_keys = |id: &[u8]| keys.get(id).cloned().ok_or(AppStateError::KeyNotFound);
    let mut mutations = Vec::new();
    if let Some(snapshot) = &list.snapshot {
        mutations.extend(process_snapshot(snapshot, state, get_keys, false, COLLECTION)?.mutations);
    }
    for patch in &list.patches {
        let processed = process_patch(patch, state, get_keys, |_| Ok(None), false, COLLECTION)?;
        mutations.extend(processed.mutations);
    }
    Ok(mutations)
}

/// Keeps `sticker` among the account's favorites, or takes it out, on all
/// its devices.
pub(crate) async fn set_favorite(
    client: Arc<Client>,
    sticker: Sticker,
    favorite: bool,
) -> Result<(), ActionError> {
    let media = &sticker.media;
    let value = wa::SyncActionValue {
        sticker_action: wa::sync_action_value::StickerAction {
            file_enc_sha256: Some(media.file_enc_sha256.clone()),
            media_key: Some(media.media_key.clone()),
            mimetype: media.mime.clone(),
            height: media.height,
            width: media.width,
            direct_path: Some(media.direct_path.clone()),
            file_length: media.size,
            is_favorite: Some(favorite),
            is_lottie: Some(sticker.lottie),
            ..Default::default()
        }
        .into(),
        timestamp: Some(now_ms()),
        ..Default::default()
    };
    client
        .send_app_state_action(&schemas::FAVORITE_STICKER, &[&sticker.hash], &value)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// Sends `sticker` to `to`. Its file is not uploaded again: the message
/// points at the copy already on WhatsApp's servers.
pub(crate) async fn send(
    client: Arc<Client>,
    to: whatsapp_rust::Jid,
    sticker: Sticker,
) -> Result<String, ActionError> {
    let message = media_message(ContentKind::Sticker, &sticker.media, None, None);
    client
        .send_message(to, message)
        .await
        .map(|sent| sent.message_id)
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// One page of the collection: its snapshot and the patches after it on
/// the first page, the patches after `version` on later ones.
async fn fetch_page(client: &Client, version: Option<u64>) -> Result<PatchList, ActionError> {
    let mut collection = NodeBuilder::new("collection")
        .attr("name", COLLECTION)
        .attr(
            "return_snapshot",
            if version.is_none() { "true" } else { "false" },
        );
    if let Some(version) = version {
        collection = collection.attr("version", version);
    }
    let sync = NodeBuilder::new("sync")
        .children([collection.build()])
        .build();
    let answer = client
        .send_iq(InfoQuery {
            namespace: "w:sync:app:state",
            query_type: InfoQueryType::Set,
            to: server_jid().clone(),
            target: None,
            id: None,
            content: Some(NodeContent::Nodes(vec![sync])),
            timeout: None,
        })
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    parse_patch_list_ref(answer.get()).map_err(|error| ActionError::Rejected(error.to_string()))
}

/// Downloads the snapshot and the mutations kept apart from their
/// patches, and puts them in place.
async fn inline_blobs(client: &Client, list: &mut PatchList) -> Result<(), ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    if let Some(blob) = list.snapshot_ref.take() {
        let bytes = client
            .download(&blob)
            .await
            .map_err(|error| rejected(&error))?;
        list.snapshot =
            Some(wa::SyncdSnapshot::decode_from_slice(&bytes).map_err(|error| rejected(&error))?);
    }
    for patch in &mut list.patches {
        if let Some(blob) = patch.external_mutations.as_option().cloned() {
            let bytes = client
                .download(&blob)
                .await
                .map_err(|error| rejected(&error))?;
            let external =
                wa::SyncdMutations::decode_from_slice(&bytes).map_err(|error| rejected(&error))?;
            patch.mutations = external.mutations;
        }
    }
    Ok(())
}

/// The keys the page's records are encrypted with, from whatsapp-rust's
/// store, where the phone's key shares land.
async fn keys_for(
    client: &Client,
    list: &PatchList,
) -> Result<HashMap<Vec<u8>, Arc<ExpandedAppStateKeys>>, ActionError> {
    let store = client.persistence_manager().backend();
    let mut keys = HashMap::new();
    for id in key_ids(list) {
        if keys.contains_key(&id) {
            continue;
        }
        let key = store
            .get_sync_key(&id)
            .await
            .map_err(|error| ActionError::Rejected(error.to_string()))?
            .ok_or_else(|| {
                ActionError::Rejected(format!("no app state key {}", BASE64.encode(&id)))
            })?;
        keys.insert(id, Arc::new(expand_app_state_keys(&key.key_data)));
    }
    Ok(keys)
}

/// Every key id a page's snapshot and patches name.
fn key_ids(list: &PatchList) -> Vec<Vec<u8>> {
    let id_of = |key: &whatsapp_rust::buffa::MessageField<wa::KeyId>| {
        key.as_option().and_then(|key| key.id.clone())
    };
    let snapshot = list.snapshot.iter().flat_map(|snapshot| {
        id_of(&snapshot.key_id).into_iter().chain(
            snapshot
                .records
                .iter()
                .filter_map(|record| id_of(&record.key_id)),
        )
    });
    let patches = list.patches.iter().flat_map(|patch| {
        id_of(&patch.key_id).into_iter().chain(
            patch
                .mutations
                .iter()
                .filter_map(|mutation| mutation.record.as_option())
                .filter_map(|record| id_of(&record.key_id)),
        )
    });
    snapshot.chain(patches).collect()
}

/// The favorite stickers `mutations` leave, applied in order: a sticker
/// marked favorite joins, one unmarked or removed leaves.
fn favorites_in(mutations: &[Mutation]) -> Vec<Sticker> {
    let mut favorites: Vec<Sticker> = Vec::new();
    for mutation in mutations {
        let (Some(kind), Some(hash)) = (mutation.index.first(), mutation.index.get(1)) else {
            continue;
        };
        if kind != FAVORITE {
            continue;
        }
        favorites.retain(|sticker| sticker.hash != *hash);
        let action = mutation
            .action_value
            .as_ref()
            .and_then(|value| value.sticker_action.as_option());
        let kept = mutation.operation == wa::syncd_mutation::SyncdOperation::Set
            && action.is_some_and(|action| action.is_favorite == Some(true));
        if let Some(action) = action.filter(|_| kept)
            && let Some(sticker) = sticker(hash, action)
        {
            favorites.push(sticker);
        }
    }
    favorites
}

/// The sticker a favorite's action describes, when it says where its file
/// is.
fn sticker(hash: &str, action: &wa::sync_action_value::StickerAction) -> Option<Sticker> {
    let lottie = action.is_lottie == Some(true);
    Some(Sticker {
        hash: hash.to_owned(),
        lottie,
        media: Media {
            mime: action.mimetype.clone().or_else(|| {
                Some(
                    if lottie {
                        "application/x-lottie"
                    } else {
                        "image/webp"
                    }
                    .to_owned(),
                )
            }),
            width: action.width,
            height: action.height,
            size: action.file_length,
            direct_path: action.direct_path.clone()?,
            media_key: action.media_key.clone()?,
            file_sha256: BASE64.decode(hash).ok()?,
            file_enc_sha256: action.file_enc_sha256.clone()?,
            ..Media::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use base64::Engine as _;
    use whatsapp_rust::wacore::appstate::hash::HashState;
    use whatsapp_rust::wacore::appstate::patch_decode::{PatchList, WAPatchName};
    use whatsapp_rust::wacore::appstate::{
        AppStateError, Mutation, encode_record, expand_app_state_keys,
    };
    use whatsapp_rust::waproto::whatsapp as wa;

    use super::{BASE64, decode_page, favorites_in, key_ids};

    fn mark(hash: &str, favorite: bool, operation: wa::syncd_mutation::SyncdOperation) -> Mutation {
        Mutation {
            action_value: Some(wa::SyncActionValue {
                sticker_action: wa::sync_action_value::StickerAction {
                    direct_path: Some(format!("/v/{hash}")),
                    media_key: Some(vec![1; 32]),
                    file_enc_sha256: Some(vec![2; 32]),
                    mimetype: Some("image/webp".into()),
                    is_favorite: Some(favorite),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            }),
            index: vec!["favoriteSticker".into(), hash.into()],
            operation,
        }
    }

    fn hash(byte: u8) -> String {
        BASE64.encode([byte; 32])
    }

    fn hashes(mutations: &[Mutation]) -> Vec<String> {
        favorites_in(mutations)
            .into_iter()
            .map(|sticker| sticker.hash)
            .collect()
    }

    #[test]
    fn keeps_stickers_marked_favorite_in_order() {
        let set = wa::syncd_mutation::SyncdOperation::Set;
        assert_eq!(
            hashes(&[mark(&hash(1), true, set), mark(&hash(2), true, set)]),
            [hash(1), hash(2)]
        );
    }

    #[test]
    fn drops_stickers_unmarked_or_removed_later() {
        let set = wa::syncd_mutation::SyncdOperation::Set;
        let remove = wa::syncd_mutation::SyncdOperation::Remove;
        assert_eq!(
            hashes(&[
                mark(&hash(1), true, set),
                mark(&hash(2), true, set),
                mark(&hash(3), true, set),
                mark(&hash(1), false, set),
                mark(&hash(2), true, remove),
            ]),
            [hash(3)]
        );
    }

    #[test]
    fn moves_a_sticker_marked_again_to_the_end() {
        let set = wa::syncd_mutation::SyncdOperation::Set;
        assert_eq!(
            hashes(&[
                mark(&hash(1), true, set),
                mark(&hash(2), true, set),
                mark(&hash(1), true, set),
            ]),
            [hash(2), hash(1)]
        );
    }

    #[test]
    fn reads_where_the_file_is_and_its_hash() {
        let set = wa::syncd_mutation::SyncdOperation::Set;
        let favorites = favorites_in(&[mark(&hash(7), true, set)]);
        assert_eq!(favorites[0].media.direct_path, format!("/v/{}", hash(7)));
        assert_eq!(favorites[0].media.file_sha256, vec![7; 32]);
    }

    /// A first page as the server sends it: a snapshot whose favorite
    /// sticker record is encrypted with `key_id`'s key.
    fn encrypted_page(key_id: &[u8], key_data: &[u8]) -> PatchList {
        let keys = expand_app_state_keys(key_data);
        let value = wa::SyncActionValue {
            sticker_action: wa::sync_action_value::StickerAction {
                direct_path: Some("/v/sticker".into()),
                media_key: Some(vec![1; 32]),
                file_enc_sha256: Some(vec![2; 32]),
                is_favorite: Some(true),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let index = format!(r#"["favoriteSticker","{}"]"#, hash(9));
        let (mutation, _) = encode_record(
            wa::syncd_mutation::SyncdOperation::Set,
            index.as_bytes(),
            &value,
            &keys,
            key_id,
            &[3; 16],
            7,
        );
        PatchList {
            name: WAPatchName::RegularLow,
            has_more_patches: false,
            patches: Vec::new(),
            snapshot: Some(wa::SyncdSnapshot {
                version: wa::SyncdVersion { version: Some(4) }.into(),
                records: mutation.record.into_option().into_iter().collect(),
                key_id: wa::KeyId {
                    id: Some(key_id.to_vec()),
                }
                .into(),
                ..Default::default()
            }),
            snapshot_ref: None,
            error: None,
        }
    }

    #[test]
    fn decrypts_a_favorite_from_a_snapshot() -> Result<(), AppStateError> {
        let (key_id, key_data) = (b"key-1".to_vec(), vec![5; 32]);
        let page = encrypted_page(&key_id, &key_data);
        assert_eq!(key_ids(&page)[0], key_id);
        let keys = HashMap::from([(key_id, Arc::new(expand_app_state_keys(&key_data)))]);
        let mutations = decode_page(&page, &keys, &mut HashState::default())?;
        let favorites = favorites_in(&mutations);
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0].hash, hash(9));
        assert_eq!(favorites[0].media.direct_path, "/v/sticker");
        Ok(())
    }

    #[test]
    fn fails_without_the_key_the_page_needs() {
        let page = encrypted_page(b"key-1", &[5; 32]);
        let result = decode_page(&page, &HashMap::new(), &mut HashState::default());
        assert!(matches!(result, Err(AppStateError::KeyNotFound)));
    }

    #[test]
    fn ignores_other_app_state() {
        let set = wa::syncd_mutation::SyncdOperation::Set;
        let mut other = mark(&hash(1), true, set);
        other.index = vec!["pin_v1".into(), "1@lid".into()];
        assert_eq!(hashes(&[other]), Vec::<String>::new());
    }
}
