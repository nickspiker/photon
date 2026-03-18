//! Flat provenance-hash storage with zero metadata leakage.
//!
//! All files stored in ~/.config/photon/ with base64-encoded provenance-hash names.
//! No hierarchical directories, no predictable names, no metadata leakage.
//!
//! Storage architecture:
//! - Contact list: Single bootstrap file with derived filename
//! - Contact blobs: One file per contact (static metadata + CLUTCH state)
//! - Chain blobs: One file per contact (hot path - chain links + message index)
//! - Avatar files: One file per contact (raw VSF bytes, rarely changes)
//! - Messages: Separate per-message files (per CHAIN.md spec)
//!
//! Filename derivation: hash + spaghettify + base64
//! All keys derived from: identity_seed + device_secret + domain string

use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::path::PathBuf;
use vsf::schema::{FromVsfType, SectionBuilder, SectionSchema, TypeConstraint};
use vsf::{VsfSection, VsfType};

use crate::crypto::clutch::spaghettify;
use crate::storage::contacts::StorageError;
use crate::types::FriendshipId;

// ============================================================================
// Flat Storage Core
// ============================================================================

/// Flat provenance-hash storage manager
/// All files stored in single flat directory with opaque base64 names
pub struct FlatStorage {
    root: PathBuf, // ~/.config/photon/ - ALL files go here
    identity_seed: [u8; 32],
    device_secret: [u8; 32],
}

impl FlatStorage {
    /// Create new flat storage instance
    pub fn new(
        identity_seed: [u8; 32],
        device_secret: [u8; 32],
    ) -> Result<Self, StorageError> {
        let root = photon_flat_dir()?;
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            identity_seed,
            device_secret,
        })
    }

    // ========================================================================
    // Contact List (bootstrap file)
    // ========================================================================

    /// Load contact list from disk
    pub fn load_contact_list(&self) -> Result<Vec<ContactListEntry>, StorageError> {
        let filename = derive_contact_list_filename(&self.identity_seed, &self.device_secret);
        let path = self.root.join(&filename);

        if !path.exists() {
            return Ok(Vec::new());
        }

        let encrypted = super::read_file(&path, &format!("flat/contact_list/{}", filename))?;
        let key = derive_contact_list_key(&self.identity_seed, &self.device_secret);
        let vsf_bytes = decrypt_data(&encrypted, &key)?;

        let schema = contact_list_schema();
        let builder = SectionBuilder::parse(schema, &vsf_bytes)
            .map_err(|e| StorageError::Parse(format!("Contact list parse: {}", e)))?;

        let mut contacts = Vec::new();
        for field in builder.get_fields("contact") {
            if field.values.len() >= 2 {
                let handle = match &field.values[0] {
                    VsfType::x(s) => s.clone(),
                    _ => continue,
                };
                let identity_seed: [u8; 32] = match &field.values[1] {
                    VsfType::hb(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                    _ => continue,
                };

                contacts.push(ContactListEntry {
                    handle,
                    identity_seed,
                });
            }
        }

        Ok(contacts)
    }

    /// Save contact list to disk
    pub fn save_contact_list(&self, contacts: &[ContactListEntry]) -> Result<(), StorageError> {
        let schema = contact_list_schema();
        let mut builder = schema.build();

        for c in contacts {
            builder = builder
                .append_multi(
                    "contact",
                    vec![
                        VsfType::x(c.handle.clone()),
                        VsfType::hb(c.identity_seed.to_vec()),
                    ],
                )
                .map_err(|e| StorageError::Parse(e.to_string()))?;
        }

        let vsf_bytes = builder
            .encode()
            .map_err(|e| StorageError::Parse(e.to_string()))?;

        let key = derive_contact_list_key(&self.identity_seed, &self.device_secret);
        let encrypted = encrypt_data(&vsf_bytes, &key, "encrypted_contact_list")?;

        let filename = derive_contact_list_filename(&self.identity_seed, &self.device_secret);
        let path = self.root.join(&filename);
        super::write_file(
            &path,
            &encrypted,
            &format!("flat/contact_list/{}", filename),
            super::WritePolicy::BestEffort,
        )?;

        #[cfg(feature = "development")]
        crate::log(&format!(
            "FLAT: Saved contact list ({} contacts) to {}",
            contacts.len(),
            filename
        ));

        Ok(())
    }

    // ========================================================================
    // Contact Blob (static metadata + CLUTCH state)
    // ========================================================================

    /// Load contact blob from disk
    pub fn load_contact_blob(
        &self,
        their_identity_seed: &[u8; 32],
    ) -> Result<ContactBlob, StorageError> {
        let filename = derive_contact_blob_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if !path.exists() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Contact blob not found",
            )));
        }

        let encrypted = super::read_file(&path, &format!("flat/contact_blob/{}", filename))?;
        let key =
            derive_contact_blob_key(&self.identity_seed, their_identity_seed, &self.device_secret);
        let vsf_bytes = decrypt_data(&encrypted, &key)?;

        let mut ptr = 0;
        let section = VsfSection::parse(&vsf_bytes, &mut ptr)
            .map_err(|e| StorageError::Parse(format!("Contact blob parse: {}", e)))?;

        let get_val = |name: &str| -> Option<&VsfType> { section.get_field(name)?.values.first() };

        let identity_seed: [u8; 32] = match get_val("identity_seed") {
            Some(VsfType::hb(v)) if v.len() == 32 => v.as_slice().try_into().unwrap(),
            _ => return Err(StorageError::Parse("Missing identity_seed".into())),
        };

        let handle = match get_val("handle") {
            Some(VsfType::x(s)) => s.clone(),
            _ => return Err(StorageError::Parse("Missing handle".into())),
        };

        let added = match get_val("added") {
            Some(VsfType::e(vsf::types::EtType::i(osc))) => *osc as u64,
            _ => 0,
        };

        let friendship_id = match get_val("friendship_id") {
            Some(VsfType::hb(v)) if v.len() == 32 => {
                Some(FriendshipId::from_bytes(v.as_slice().try_into().unwrap()))
            }
            _ => None,
        };

        let relationship_seed = match get_val("relationship_seed") {
            Some(VsfType::hb(v)) if v.len() == 32 => Some(v.as_slice().try_into().unwrap()),
            _ => None,
        };

        let trust_level = match get_val("trust_level") {
            Some(VsfType::x(s)) => s.clone(),
            _ => "Stranger".to_string(),
        };

        // Load devices (multi-device support)
        let mut devices = Vec::new();
        if let Some(devices_field) = section.get_field("devices") {
            for device_val in &devices_field.values {
                if let VsfType::v(b'D', device_bytes) = device_val {
                    let mut ptr = 0;
                    let device_section = match VsfSection::parse(device_bytes, &mut ptr) {
                        Ok(sec) => sec,
                        Err(_) => continue,
                    };
                    let get_dev = |name: &str| -> Option<&VsfType> {
                        device_section.get_field(name)?.values.first()
                    };

                    let device_id: [u8; 32] = match get_dev("device_id") {
                        Some(VsfType::hb(v)) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                        _ => continue,
                    };

                    let device_pubkey: [u8; 32] = match get_dev("device_pubkey") {
                        Some(VsfType::hb(v)) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                        _ => continue,
                    };

                    let ip = match get_dev("ip") {
                        Some(VsfType::x(s)) => Some(s.clone()),
                        _ => None,
                    };

                    let local_ip = match get_dev("local_ip") {
                        Some(VsfType::hb(v)) if v.len() == 4 => {
                            Some([v[0], v[1], v[2], v[3]])
                        }
                        _ => None,
                    };

                    let local_port = match get_dev("local_port") {
                        Some(VsfType::i(n)) => Some(*n as u16),
                        _ => None,
                    };

                    let last_seen = match get_dev("last_seen") {
                        Some(VsfType::e(vsf::types::EtType::i(osc))) => Some(*osc as u64),
                        _ => None,
                    };

                    let clutch_state = match get_dev("clutch_state") {
                        Some(VsfType::x(s)) => s.clone(),
                        _ => "Pending".to_string(),
                    };

                    let keypairs = match get_dev("keypairs") {
                        Some(VsfType::v(b'C', data)) => Some(data.clone()),
                        _ => None,
                    };

                    let slots = match get_dev("slots") {
                        Some(VsfType::v(b'C', data)) => Some(data.clone()),
                        _ => None,
                    };

                    let ceremony_id = match get_dev("ceremony_id") {
                        Some(VsfType::hb(v)) if v.len() == 32 => {
                            Some(v.as_slice().try_into().unwrap())
                        }
                        _ => None,
                    };

                    let offer_provenances: Vec<[u8; 32]> = device_section
                        .get_field("offer_provenances")
                        .map(|f| {
                            f.values
                                .iter()
                                .filter_map(|v| match v {
                                    VsfType::hb(b) if b.len() == 32 => {
                                        Some(b.as_slice().try_into().unwrap())
                                    }
                                    _ => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    let completed_their_hqc_prefix = match get_dev("completed_their_hqc_prefix") {
                        Some(VsfType::hb(v)) if v.len() == 8 => {
                            Some(v.as_slice().try_into().unwrap())
                        }
                        _ => None,
                    };

                    devices.push(DeviceBlob {
                        device_id,
                        device_pubkey,
                        ip,
                        local_ip,
                        local_port,
                        last_seen,
                        clutch_state,
                        keypairs,
                        slots,
                        ceremony_id,
                        offer_provenances,
                        completed_their_hqc_prefix,
                    });
                }
            }
        }

        Ok(ContactBlob {
            identity_seed,
            handle,
            added,
            friendship_id,
            relationship_seed,
            trust_level,
            devices,
        })
    }

    /// Save contact blob to disk
    pub fn save_contact_blob(&self, blob: &ContactBlob) -> Result<(), StorageError> {
        let mut section = VsfSection::new("contact_blob");

        section.add_field("identity_seed", VsfType::hb(blob.identity_seed.to_vec()));
        section.add_field("handle", VsfType::x(blob.handle.clone()));
        section.add_field("added", VsfType::e(vsf::types::EtType::i(blob.added as i64)));

        if let Some(ref friendship_id) = blob.friendship_id {
            section.add_field(
                "friendship_id",
                VsfType::hb(friendship_id.as_bytes().to_vec()),
            );
        }

        if let Some(ref relationship_seed) = blob.relationship_seed {
            section.add_field("relationship_seed", VsfType::hb(relationship_seed.to_vec()));
        }

        section.add_field("trust_level", VsfType::x(blob.trust_level.clone()));

        // Save devices
        if !blob.devices.is_empty() {
            let device_values: Vec<VsfType> = blob
                .devices
                .iter()
                .map(|device| {
                    let mut device_section = VsfSection::new("device");
                    device_section.add_field("device_id", VsfType::hb(device.device_id.to_vec()));
                    device_section.add_field("device_pubkey", VsfType::hb(device.device_pubkey.to_vec()));

                    if let Some(ref ip) = device.ip {
                        device_section.add_field("ip", VsfType::x(ip.clone()));
                    }

                    if let Some(local_ip) = device.local_ip {
                        device_section.add_field("local_ip", VsfType::hb(local_ip.to_vec()));
                    }

                    if let Some(local_port) = device.local_port {
                        device_section.add_field("local_port", VsfType::i(local_port as isize));
                    }

                    if let Some(last_seen) = device.last_seen {
                        device_section.add_field(
                            "last_seen",
                            VsfType::e(vsf::types::EtType::i(last_seen as i64)),
                        );
                    }

                    device_section.add_field("clutch_state", VsfType::x(device.clutch_state.clone()));

                    if let Some(ref keypairs) = device.keypairs {
                        device_section.add_field("keypairs", VsfType::v(b'C', keypairs.clone()));
                    }

                    if let Some(ref slots) = device.slots {
                        device_section.add_field("slots", VsfType::v(b'C', slots.clone()));
                    }

                    if let Some(ceremony_id) = device.ceremony_id {
                        device_section.add_field("ceremony_id", VsfType::hb(ceremony_id.to_vec()));
                    }

                    if !device.offer_provenances.is_empty() {
                        device_section.add_field_multi(
                            "offer_provenances",
                            device
                                .offer_provenances
                                .iter()
                                .map(|p| VsfType::hb(p.to_vec()))
                                .collect(),
                        );
                    }

                    if let Some(prefix) = device.completed_their_hqc_prefix {
                        device_section.add_field("completed_their_hqc_prefix", VsfType::hb(prefix.to_vec()));
                    }

                    VsfType::v(b'D', device_section.encode())
                })
                .collect();

            section.add_field_multi("devices", device_values);
        }

        let vsf_bytes = section.encode();

        let key = derive_contact_blob_key(
            &self.identity_seed,
            &blob.identity_seed,
            &self.device_secret,
        );
        let encrypted = encrypt_data(&vsf_bytes, &key, "encrypted_contact_blob")?;

        let filename = derive_contact_blob_filename(
            &self.identity_seed,
            &blob.identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);
        super::write_file(
            &path,
            &encrypted,
            &format!("flat/contact_blob/{}", filename),
            super::WritePolicy::BestEffort,
        )?;

        #[cfg(feature = "development")]
        crate::log(&format!(
            "FLAT: Saved contact blob for {} to {}",
            blob.handle, filename
        ));

        Ok(())
    }

    /// Delete contact blob from disk
    pub fn delete_contact_blob(&self, their_identity_seed: &[u8; 32]) -> Result<(), StorageError> {
        let filename = derive_contact_blob_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if path.exists() {
            fs::remove_file(&path)?;
            #[cfg(feature = "development")]
            crate::log(&format!("FLAT: Deleted contact blob {}", filename));
        }

        Ok(())
    }

    // ========================================================================
    // Chain Blob (hot path - chain links + message index)
    // ========================================================================

    /// Load chain blob from disk
    pub fn load_chain_blob(
        &self,
        their_identity_seed: &[u8; 32],
    ) -> Result<ChainBlob, StorageError> {
        let filename = derive_chain_blob_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if !path.exists() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Chain blob not found",
            )));
        }

        let encrypted = super::read_file(&path, &format!("flat/chain_blob/{}", filename))?;
        let key =
            derive_chain_blob_key(&self.identity_seed, their_identity_seed, &self.device_secret);
        let vsf_bytes = decrypt_data(&encrypted, &key)?;

        let mut ptr = 0;
        let section = VsfSection::parse(&vsf_bytes, &mut ptr)
            .map_err(|e| StorageError::Parse(format!("Chain blob parse: {}", e)))?;

        let get_val = |name: &str| -> Option<&VsfType> { section.get_field(name)?.values.first() };

        let chain_links = match get_val("chain_links") {
            Some(VsfType::v(b'C', data)) => Some(data.clone()),
            _ => None,
        };

        let chain_last_ack_time = match get_val("chain_last_ack_time") {
            Some(VsfType::e(vsf::types::EtType::i(osc))) => Some(*osc),
            Some(v) => {
                // Legacy: convert any EagleTime to oscillations
                let et = vsf::types::EagleTime::new_from_vsf(v.clone());
                et.oscillations()
            },
            None => None,
        };

        let message_index: Vec<MessageIndexEntry> = section
            .get_fields("message")
            .iter()
            .filter_map(|field| {
                if field.values.len() >= 3 {
                    let network_id: [u8; 32] = match &field.values[0] {
                        VsfType::hg(v) if v.len() == 32 => v.as_slice().try_into().ok()?,
                        _ => return None,
                    };
                    let eagle_time = match &field.values[1] {
                        VsfType::e(vsf::types::EtType::i(osc)) => *osc,
                        v => {
                            let et = vsf::types::EagleTime::new_from_vsf(v.clone());
                            et.oscillations().unwrap_or(0)
                        },
                    };
                    let author_index = usize::from_vsf_type(&field.values[2]).ok()?;
                    Some(MessageIndexEntry {
                        network_id,
                        eagle_time,
                        author_index,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(ChainBlob {
            chain_links,
            chain_last_ack_time,
            message_index,
        })
    }

    /// Save chain blob to disk
    pub fn save_chain_blob(
        &self,
        blob: &ChainBlob,
        their_identity_seed: &[u8; 32],
    ) -> Result<(), StorageError> {
        let mut section = VsfSection::new("chain_blob");

        if let Some(ref chain_links) = blob.chain_links {
            section.add_field("chain_links", VsfType::v(b'C', chain_links.clone()));
        }
        if let Some(chain_last_ack_time) = blob.chain_last_ack_time {
            section.add_field(
                "chain_last_ack_time",
                VsfType::e(vsf::types::EtType::i(chain_last_ack_time)),
            );
        }

        for entry in &blob.message_index {
            section.add_field_multi(
                "message",
                vec![
                    VsfType::hg(entry.network_id.to_vec()),
                    VsfType::e(vsf::types::EtType::i(entry.eagle_time)),
                    VsfType::u(entry.author_index, false),
                ],
            );
        }

        let vsf_bytes = section.encode();

        let key =
            derive_chain_blob_key(&self.identity_seed, their_identity_seed, &self.device_secret);
        let encrypted = encrypt_data(&vsf_bytes, &key, "encrypted_chain_blob")?;

        let filename = derive_chain_blob_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);
        super::write_file(
            &path,
            &encrypted,
            &format!("flat/chain_blob/{}", filename),
            super::WritePolicy::MustSucceed,
        )?;

        #[cfg(feature = "development")]
        crate::log(&format!("FLAT: Saved chain blob to {}", filename));

        Ok(())
    }

    /// Delete chain blob from disk
    pub fn delete_chain_blob(&self, their_identity_seed: &[u8; 32]) -> Result<(), StorageError> {
        let filename = derive_chain_blob_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if path.exists() {
            fs::remove_file(&path)?;
            #[cfg(feature = "development")]
            crate::log(&format!("FLAT: Deleted chain blob {}", filename));
        }

        Ok(())
    }

    // ========================================================================
    // Avatar File (separate, rarely changes)
    // ========================================================================

    /// Load avatar from disk
    pub fn load_avatar(&self, their_identity_seed: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
        let filename = derive_avatar_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if !path.exists() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Avatar not found",
            )));
        }

        let encrypted = super::read_file(&path, &format!("flat/avatar/{}", filename))?;
        let key =
            derive_avatar_key(&self.identity_seed, their_identity_seed, &self.device_secret);
        decrypt_data(&encrypted, &key)
    }

    /// Save avatar to disk
    pub fn save_avatar(
        &self,
        vsf_bytes: &[u8],
        their_identity_seed: &[u8; 32],
    ) -> Result<(), StorageError> {
        let key =
            derive_avatar_key(&self.identity_seed, their_identity_seed, &self.device_secret);
        let encrypted = encrypt_data(vsf_bytes, &key, "encrypted_avatar")?;

        let filename = derive_avatar_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);
        super::write_file(
            &path,
            &encrypted,
            &format!("flat/avatar/{}", filename),
            super::WritePolicy::BestEffort,
        )?;

        #[cfg(feature = "development")]
        crate::log(&format!("FLAT: Saved avatar to {}", filename));

        Ok(())
    }

    /// Delete avatar from disk
    pub fn delete_avatar(&self, their_identity_seed: &[u8; 32]) -> Result<(), StorageError> {
        let filename = derive_avatar_filename(
            &self.identity_seed,
            their_identity_seed,
            &self.device_secret,
        );
        let path = self.root.join(&filename);

        if path.exists() {
            fs::remove_file(&path)?;
            #[cfg(feature = "development")]
            crate::log(&format!("FLAT: Deleted avatar {}", filename));
        }

        Ok(())
    }

    // ========================================================================
    // Messages (per-message files)
    // ========================================================================

    /// Load message from disk
    pub fn load_message(&self, network_id: &[u8; 32]) -> Result<MessageBlob, StorageError> {
        let filename = derive_message_filename(network_id, &self.identity_seed, &self.device_secret);
        let path = self.root.join(&filename);

        if !path.exists() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Message not found",
            )));
        }

        let encrypted = super::read_file(&path, &format!("flat/message/{}", filename))?;
        let key = derive_message_key(&self.identity_seed, &self.device_secret);
        let vsf_bytes = decrypt_data(&encrypted, &key)?;

        let mut ptr = 0;
        let section = VsfSection::parse(&vsf_bytes, &mut ptr)
            .map_err(|e| StorageError::Parse(format!("Message parse: {}", e)))?;

        let get_val = |name: &str| -> Option<&VsfType> { section.get_field(name)?.values.first() };

        let author_index = get_val("author_index")
            .ok_or_else(|| StorageError::Parse("Missing author_index field".into()))?;
        let author_index = usize::from_vsf_type(author_index)
            .map_err(|e| StorageError::Parse(format!("Invalid author_index: {}", e)))?;

        let status = get_val("status")
            .map(|v| {
                u8::from_vsf_type(v).unwrap_or_else(|e| {
                    crate::log(&format!("STORAGE: Failed to parse message status: {}", e));
                    0
                })
            })
            .unwrap_or(0);

        let eagle_time = match get_val("eagle_time") {
            Some(VsfType::e(vsf::types::EtType::i(osc))) => *osc,
            Some(v) => {
                let et = vsf::types::EagleTime::new_from_vsf(v.clone());
                et.oscillations().unwrap_or(0)
            },
            None => 0,
        };

        let plaintext = match get_val("plaintext") {
            Some(VsfType::x(s)) => s.clone(),
            _ => String::new(),
        };

        let wire_format = match get_val("wire_format") {
            Some(VsfType::v(b'C', data)) => Some(data.clone()),
            _ => None,
        };

        let prev_msg_hp = match get_val("prev_msg_hp") {
            Some(VsfType::hp(v)) if v.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(v);
                Some(arr)
            }
            _ => None,
        };

        Ok(MessageBlob {
            author_index,
            status,
            eagle_time,
            plaintext,
            wire_format,
            prev_msg_hp,
        })
    }

    /// Save message to disk
    pub fn save_message(
        &self,
        blob: &MessageBlob,
        network_id: &[u8; 32],
    ) -> Result<(), StorageError> {
        let mut section = VsfSection::new("message");

        section.add_field("author_index", VsfType::u(blob.author_index, false));
        section.add_field("status", VsfType::u(blob.status as usize, false));
        section.add_field(
            "eagle_time",
            VsfType::e(vsf::types::EtType::i(blob.eagle_time)),
        );
        section.add_field("plaintext", VsfType::x(blob.plaintext.clone()));

        if let Some(ref wire_format) = blob.wire_format {
            section.add_field("wire_format", VsfType::v(b'C', wire_format.clone()));
        }

        if let Some(ref prev_msg_hp) = blob.prev_msg_hp {
            section.add_field("prev_msg_hp", VsfType::hp(prev_msg_hp.to_vec()));
        }

        let vsf_bytes = section.encode();

        let key = derive_message_key(&self.identity_seed, &self.device_secret);
        let encrypted = encrypt_data(&vsf_bytes, &key, "encrypted_message")?;

        let filename = derive_message_filename(network_id, &self.identity_seed, &self.device_secret);
        let path = self.root.join(&filename);
        super::write_file(
            &path,
            &encrypted,
            &format!("flat/message/{}", filename),
            super::WritePolicy::BestEffort,
        )?;

        #[cfg(feature = "development")]
        crate::log(&format!("FLAT: Saved message to {}", filename));

        Ok(())
    }

    /// Delete message from disk
    pub fn delete_message(&self, network_id: &[u8; 32]) -> Result<(), StorageError> {
        let filename = derive_message_filename(network_id, &self.identity_seed, &self.device_secret);
        let path = self.root.join(&filename);

        if path.exists() {
            fs::remove_file(&path)?;
            #[cfg(feature = "development")]
            crate::log(&format!("FLAT: Deleted message {}", filename));
        }

        Ok(())
    }
}

// ============================================================================
// Filename Derivation (all use hash + spaghettify + base64)
// ============================================================================

/// Derive contact list filename
fn derive_contact_list_filename(identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_list_v1");
    hasher.update(identity_seed);
    hasher.update(device_secret);
    let hp1 = hasher.finalize();
    let hp2 = spaghettify(hp1.as_bytes());
    format!("{}.vsf", base64url_encode(&hp2))
}

/// Derive contact blob filename
fn derive_contact_blob_filename(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_blob_v2"); // v2 - separated structure
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    let hp1 = hasher.finalize();
    let hp2 = spaghettify(hp1.as_bytes());
    format!("{}.vsf", base64url_encode(&hp2))
}

/// Derive chain blob filename
fn derive_chain_blob_filename(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_chain_blob_v1");
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    let hp1 = hasher.finalize();
    let hp2 = spaghettify(hp1.as_bytes());
    format!("{}.vsf", base64url_encode(&hp2))
}

/// Derive avatar filename
fn derive_avatar_filename(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_avatar_v1");
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    let hp1 = hasher.finalize();
    let hp2 = spaghettify(hp1.as_bytes());
    format!("{}.vsf", base64url_encode(&hp2))
}

/// Derive message filename (now uses spaghettify like everything else)
fn derive_message_filename(
    network_id: &[u8; 32],
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_message_v1");
    hasher.update(network_id);
    hasher.update(identity_seed);
    hasher.update(device_secret);
    let hp1 = hasher.finalize();
    let hp2 = spaghettify(hp1.as_bytes());
    format!("{}.vsf", base64url_encode(&hp2))
}

/// Base64 URL-safe encoding (no padding)
fn base64url_encode(data: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(data)
}

// ============================================================================
// Encryption Key Derivation
// ============================================================================

fn derive_contact_list_key(identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_list_key_v1");
    hasher.update(identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

fn derive_contact_blob_key(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_blob_key_v2"); // v2 - separated structure
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

fn derive_chain_blob_key(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_chain_blob_key_v1");
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

fn derive_avatar_key(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_avatar_key_v1");
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

fn derive_message_key(identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_message_key_v1");
    hasher.update(identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

// ============================================================================
// Encryption/Decryption
// ============================================================================

/// Encrypt VSF section bytes with ChaCha20-Poly1305, wrapped in proper VSF file
fn encrypt_data(data: &[u8], key: &[u8; 32], section_name: &str) -> Result<Vec<u8>, StorageError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| StorageError::Encryption(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce: Nonce = nonce_bytes.into();

    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| StorageError::Encryption(e.to_string()))?;

    let mut encrypted_payload = Vec::with_capacity(12 + ciphertext.len());
    encrypted_payload.extend_from_slice(&nonce_bytes);
    encrypted_payload.extend_from_slice(&ciphertext);

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section(
            section_name,
            vec![("data".to_string(), VsfType::v(b'e', encrypted_payload))],
        )
        .build()
        .map_err(|e| StorageError::Encryption(format!("VSF build: {}", e)))?;

    Ok(vsf_bytes)
}

/// Decrypt data from VSF-wrapped encrypted file
fn decrypt_data(vsf_bytes: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    let header = vsf::verification::parse_full_header(vsf_bytes)
        .map_err(|e| StorageError::Decryption(format!("VSF parse: {}", e)))?;

    let section_field = header
        .fields
        .first()
        .ok_or_else(|| StorageError::Decryption("No sections in VSF".to_string()))?;

    let offset = section_field.offset_bytes;
    let size = section_field.size_bytes;

    if offset + size > vsf_bytes.len() {
        return Err(StorageError::Decryption("Section beyond file".to_string()));
    }

    let mut ptr = offset;
    let section = VsfSection::parse(vsf_bytes, &mut ptr)
        .map_err(|e| StorageError::Decryption(format!("Section parse: {}", e)))?;

    let data_field = section
        .get_field("data")
        .ok_or_else(|| StorageError::Decryption("No data field".to_string()))?;

    let encrypted_payload = match data_field.values.first() {
        Some(VsfType::v(b'e', payload)) => payload,
        _ => {
            return Err(StorageError::Decryption(
                "Expected ve{} encrypted data".to_string(),
            ))
        }
    };

    if encrypted_payload.len() < 12 + 16 {
        return Err(StorageError::Decryption(
            "Encrypted payload too short".to_string(),
        ));
    }

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| StorageError::Decryption(e.to_string()))?;

    let nonce_bytes: [u8; 12] = encrypted_payload[..12]
        .try_into()
        .map_err(|_| StorageError::Decryption("Invalid nonce".to_string()))?;
    let nonce: Nonce = nonce_bytes.into();
    let ciphertext = &encrypted_payload[12..];

    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| StorageError::Decryption(e.to_string()))
}

// ============================================================================
// Schema Definitions
// ============================================================================

fn contact_list_schema() -> SectionSchema {
    SectionSchema::new("contact_list")
        .field("contact", TypeConstraint::Any) // Multi-value: (handle: x, identity_seed: hb)
}

// ============================================================================
// Data Structures
// ============================================================================

/// Entry in contact list (bootstrap file)
#[derive(Clone, Debug)]
pub struct ContactListEntry {
    pub handle: String,
    pub identity_seed: [u8; 32],
}

/// Device metadata for multi-device contacts
/// Each contact can have multiple devices, each with independent CLUTCH state
#[derive(Clone, Debug)]
pub struct DeviceBlob {
    pub device_id: [u8; 32],       // BLAKE3(device_pubkey)
    pub device_pubkey: [u8; 32],
    pub ip: Option<String>,        // Serialized SocketAddr
    pub local_ip: Option<[u8; 4]>,
    pub local_port: Option<u16>,
    pub last_seen: Option<u64>,
    pub clutch_state: String,      // "Pending" | "AwaitingProof" | "Complete"
    pub keypairs: Option<Vec<u8>>, // Serialized ClutchAllKeypairs
    pub slots: Option<Vec<u8>>,    // Serialized Vec<PartySlot>
    pub ceremony_id: Option<[u8; 32]>,
    pub offer_provenances: Vec<[u8; 32]>,
    pub completed_their_hqc_prefix: Option<[u8; 8]>,
}

/// Contact blob (static metadata + CLUTCH state)
/// One contact per handle, with multiple devices
#[derive(Clone, Debug)]
pub struct ContactBlob {
    pub identity_seed: [u8; 32],   // handle_hash (contact identifier)
    pub handle: String,
    pub added: u64,
    pub friendship_id: Option<FriendshipId>,
    pub relationship_seed: Option<[u8; 32]>,
    pub trust_level: String,       // "Stranger" | "Known" | "Trusted" | "Inner"
    pub devices: Vec<DeviceBlob>,  // All devices for this handle
}

/// Chain blob (hot path - updated on every message)
#[derive(Clone, Debug, Default)]
pub struct ChainBlob {
    pub chain_links: Option<Vec<u8>>, // Serialized 512 links
    pub chain_last_ack_time: Option<i64>,
    pub message_index: Vec<MessageIndexEntry>,
}

/// Entry in chain blob message index
#[derive(Clone, Debug)]
pub struct MessageIndexEntry {
    pub network_id: [u8; 32],
    pub eagle_time: i64,
    pub author_index: usize, // Index into ContactBlob.participants
}

/// Message blob (per-message file)
#[derive(Clone, Debug)]
pub struct MessageBlob {
    pub author_index: usize,          // Index into ContactBlob.participants
    pub status: u8,                   // 0=pending, 1=sent, 2=delivered, 3=read
    pub eagle_time: i64,
    pub plaintext: String,
    pub wire_format: Option<Vec<u8>>, // Encrypted wire bytes for re-send
    pub prev_msg_hp: Option<[u8; 32]>, // Hash chain link
}

// ============================================================================
// Directory Helpers
// ============================================================================

/// Get flat storage root - ALL files go directly here
fn photon_flat_dir() -> Result<PathBuf, StorageError> {
    #[cfg(target_os = "android")]
    let base_dir = {
        crate::ui::avatar::get_android_data_dir().ok_or_else(|| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Android data dir not set",
            ))
        })?
    };

    #[cfg(not(target_os = "android"))]
    let base_dir = dirs::config_dir()
        .ok_or_else(|| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No config dir found",
            ))
        })?
        .join("photon");

    Ok(base_dir)
}
