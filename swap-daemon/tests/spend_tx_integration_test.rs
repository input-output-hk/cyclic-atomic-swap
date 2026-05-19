mod common;
use bitcoin::hashes::Hash;
use common::*;
use swap_daemon::{config, types::{DaemonEvent, Envelope, SwapState, TxRole, WireMessage}};

use std::sync::Once;
use tracing::info;

static TRACING: Once = Once::new();

fn init_tracing() {
    TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

/// Integration test: 3 daemons complete the full spend tx signing protocol.
///
/// Simulates a 3-party cyclic swap (1→2→3→1) where each daemon independently
/// participates in:
///   1. Leader election — commit-reveal scheme to elect a deterministic leader
///   2. Adaptor point exchange — each daemon broadcasts its public adaptor point
///   3. Refund tx signing — prerequisite before spend tx signing
///   4. Spend tx signing — 3-round MuSig2 adaptor signing for all 3 spend roles
///
/// The test bypasses TCP by injecting PeerMessage events directly into each daemon.
///
/// Verifies that after exchanging Schnorr nonces and adaptor partial signatures for
/// all 3 spend roles (Spend(1), Spend(2), Spend(3)), every daemon holds 9 valid
/// adaptor signatures. Also verifies that once adaptor secrets are revealed, the
/// adapted spend txs produce valid Schnorr signatures verifiable against the
/// tweaked aggregate pubkey.
#[tokio::test]
async fn all_daemons_sign_spend_txs() {
    let (sk1, pk1) = generate_keypair();
    let (sk2, pk2) = generate_keypair();
    let (sk3, pk3) = generate_keypair();

    let keys1 = make_keys(sk1);
    let keys2 = make_keys(sk2);
    let keys3 = make_keys(sk3);

    let mut d1 = make_daemon(
        1,
        "127.0.0.1:9401",
        pk1.clone(),
        pk2.clone(),
        pk3.clone(),
        keys1,
    );
    let mut d2 = make_daemon(
        2,
        "127.0.0.1:9402",
        pk1.clone(),
        pk2.clone(),
        pk3.clone(),
        keys2,
    );
    let mut d3 = make_daemon(
        3,
        "127.0.0.1:9403",
        pk1.clone(),
        pk2.clone(),
        pk3.clone(),
        keys3,
    );

    // --- start sessions ---
    d1.start_swap_session(1).await.unwrap();
    d2.start_swap_session(1).await.unwrap();
    d3.start_swap_session(1).await.unwrap();

    // --- exchange adaptor points ---
    // in production these are broadcast over TCP via start_swap_session
    // here we inject them directly
    let ap1 = d1
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_points
        .get(&1)
        .unwrap()
        .clone();
    let ap2 = d2
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_points
        .get(&2)
        .unwrap()
        .clone();
    let ap3 = d3
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_points
        .get(&3)
        .unwrap()
        .clone();

    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::AdaptorPoint(ap2.clone())),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;
    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::AdaptorPoint(ap3.clone())),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::AdaptorPoint(ap1.clone())),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::AdaptorPoint(ap3.clone())),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::AdaptorPoint(ap1.clone())),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::AdaptorPoint(ap2.clone())),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;

    // --- leader election ---
    let c1 = own_commitment(&d1, 1);
    let c2 = own_commitment(&d2, 2);
    let c3 = own_commitment(&d3, 3);

    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::LeaderElectionCommitment(c2)),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;
    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::LeaderElectionCommitment(c3)),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::LeaderElectionCommitment(c1)),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::LeaderElectionCommitment(c3)),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::LeaderElectionCommitment(c1)),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::LeaderElectionCommitment(c2)),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;

    // build lock txs before leader nonces trigger refund+spend signing
    swap_daemon::protocol::lock_funds::build_lock_txs(d1.sessions.get_mut(&1).unwrap(), &d1.config);
    swap_daemon::protocol::lock_funds::build_lock_txs(d2.sessions.get_mut(&1).unwrap(), &d2.config);
    swap_daemon::protocol::lock_funds::build_lock_txs(d3.sessions.get_mut(&1).unwrap(), &d3.config);

    let n1 = own_nonce(&d1, 1);
    let n2 = own_nonce(&d2, 2);
    let n3 = own_nonce(&d3, 3);

    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::LeaderElectionNonce(n2)),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;
    d1.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::LeaderElectionNonce(n3)),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::LeaderElectionNonce(n1)),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d2.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 3, WireMessage::LeaderElectionNonce(n3)),
        from: "127.0.0.1:9403".to_string(),
    })
    .await;

    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 1, WireMessage::LeaderElectionNonce(n1)),
        from: "127.0.0.1:9401".to_string(),
    })
    .await;
    d3.inject_event_for_test(DaemonEvent::PeerMessage {
        envelope: Envelope::new(1, 2, WireMessage::LeaderElectionNonce(n2)),
        from: "127.0.0.1:9402".to_string(),
    })
    .await;

    assert_eq!(
        d1.sessions.get(&1).unwrap().state,
        SwapState::RefundAndSpendTxsSigning
    );
    assert_eq!(
        d2.sessions.get(&1).unwrap().state,
        SwapState::RefundAndSpendTxsSigning
    );
    assert_eq!(
        d3.sessions.get(&1).unwrap().state,
        SwapState::RefundAndSpendTxsSigning
    );

    // --- verify all daemons elected the same leader ---
    assert_eq!(leader_id(&d1), leader_id(&d2));
    assert_eq!(leader_id(&d2), leader_id(&d3));

    // --- refund nonce exchange ---
    for role in [TxRole::Refund(1), TxRole::Refund(2), TxRole::Refund(3)] {
        let nonce1 = schnorr_nonce_for(&d1, 1, role);
        let nonce2 = schnorr_nonce_for(&d2, 2, role);
        let nonce3 = schnorr_nonce_for(&d3, 3, role);

        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
    }

    // --- refund partial sig exchange ---
    for role in [TxRole::Refund(1), TxRole::Refund(2), TxRole::Refund(3)] {
        let sig1 = partial_sig_for(&d1, 1, role);
        let sig2 = partial_sig_for(&d2, 2, role);
        let sig3 = partial_sig_for(&d3, 3, role);

        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::PartialSignature {
                    role,
                    sig: sig2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::PartialSignature {
                    role,
                    sig: sig3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::PartialSignature {
                    role,
                    sig: sig1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::PartialSignature {
                    role,
                    sig: sig3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::PartialSignature {
                    role,
                    sig: sig1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::PartialSignature {
                    role,
                    sig: sig2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
    }

    // --- verify refund tx signatures ---
    for role in [TxRole::Refund(1), TxRole::Refund(2), TxRole::Refund(3)] {
        let participant_id = match role {
            TxRole::Refund(id) => id,
            _ => panic!(),
        };

        for (i, daemon) in [&d1, &d2, &d3].iter().enumerate() {
            let session = daemon.sessions.get(&1).unwrap();
            let signed_tx_hex = session.signed_txs.get(&role)
                .unwrap_or_else(|| panic!("daemon {} missing signed refund tx for {:?}", i + 1, role));
            let signed_tx: bitcoin::Transaction =
                bitcoin::consensus::encode::deserialize_hex(signed_tx_hex).unwrap();

            let lock_tx_hex = session.lock_txs.get(&participant_id).unwrap();
            let lock_tx: bitcoin::Transaction =
                bitcoin::consensus::encode::deserialize_hex(lock_tx_hex).unwrap();
            let prevout = lock_tx.output[0].clone();

            let xonly = bitcoin::XOnlyPublicKey::from_slice(&prevout.script_pubkey.as_bytes()[2..]).unwrap();

            let mut sighash_cache = bitcoin::sighash::SighashCache::new(&signed_tx);
            let sighash = sighash_cache
                .taproot_key_spend_signature_hash(
                    0,
                    &bitcoin::sighash::Prevouts::All(&[prevout]),
                    bitcoin::sighash::TapSighashType::Default,
                )
                .unwrap();

            let sig_bytes = signed_tx.input[0].witness.iter().next()
                .expect("witness should not be empty");
            let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(sig_bytes).unwrap();
            let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());

            bitcoin::secp256k1::Secp256k1::new()
                .verify_schnorr(&sig, &msg, &xonly)
                .unwrap_or_else(|_| panic!("daemon {} refund sig invalid for {:?}", i + 1, role));
        }
    }

    // --- spend nonce exchange ---
    for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
        let nonce1 = schnorr_nonce_for(&d1, 1, role);
        let nonce2 = schnorr_nonce_for(&d2, 2, role);
        let nonce3 = schnorr_nonce_for(&d3, 3, role);

        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::SchnorrNonce {
                    role,
                    nonce: nonce2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
    }

    // --- spend partial sig exchange ---
    for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
        let sig1 = partial_sig_for(&d1, 1, role);
        let sig2 = partial_sig_for(&d2, 2, role);
        let sig3 = partial_sig_for(&d3, 3, role);

        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::PartialSignature {
                    role,
                    sig: sig2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
        d1.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::PartialSignature {
                    role,
                    sig: sig3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::PartialSignature {
                    role,
                    sig: sig1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d2.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                3,
                WireMessage::PartialSignature {
                    role,
                    sig: sig3.clone(),
                },
            ),
            from: "127.0.0.1:9403".to_string(),
        })
        .await;

        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                1,
                WireMessage::PartialSignature {
                    role,
                    sig: sig1.clone(),
                },
            ),
            from: "127.0.0.1:9401".to_string(),
        })
        .await;
        d3.inject_event_for_test(DaemonEvent::PeerMessage {
            envelope: Envelope::new(
                1,
                2,
                WireMessage::PartialSignature {
                    role,
                    sig: sig2.clone(),
                },
            ),
            from: "127.0.0.1:9402".to_string(),
        })
        .await;
    }

    // --- verify all adaptor sigs produced ---
    for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
        for (i, daemon) in [&d1, &d2, &d3].iter().enumerate() {
            let session = daemon.sessions.get(&1).unwrap();
            assert!(
                session.adaptor_sigs.contains_key(&role),
                "daemon {} missing adaptor sig for {:?}",
                i + 1,
                role
            );
            assert!(
                !session.signed_txs.contains_key(&role),
                "daemon {} should not have signed spend tx for {:?} yet",
                i + 1,
                role
            );
        }
    }

    // --- reveal secrets and adapt spend txs ---
    // extract secrets first to avoid borrow conflicts
    let secret1 = d1
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_secrets
        .get(&1)
        .unwrap()
        .clone();
    let secret2 = d2
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_secrets
        .get(&2)
        .unwrap()
        .clone();
    let secret3 = d3
        .sessions
        .get(&1)
        .unwrap()
        .adaptor_secrets
        .get(&3)
        .unwrap()
        .clone();

    for daemon in [&mut d1, &mut d2, &mut d3] {
        let config = daemon.config.clone();
        let session = daemon.sessions.get_mut(&1).unwrap();
        session.adaptor_secrets.insert(1, secret1.clone());
        session.adaptor_secrets.insert(2, secret2.clone());
        session.adaptor_secrets.insert(3, secret3.clone());
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            if session.adaptor_sigs.contains_key(&role) {
                swap_daemon::cryptography::multisig::adapt_role(session, role, &config).await;
            }
        }
    }

    // --- verify adapted signatures verify against tweaked aggregate pubkey ---
    for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
        let participant_id = match role {
            TxRole::Spend(id) => id,
            _ => panic!(),
        };

        for (i, daemon) in [&d1, &d2, &d3].iter().enumerate() {
            let session = daemon.sessions.get(&1).unwrap();

            assert!(
                session.signed_txs.contains_key(&role),
                "daemon {} missing signed spend tx for {:?} after adaptation",
                i + 1,
                role
            );

            let signed_tx_hex = session.signed_txs.get(&role).unwrap();
            let signed_tx: bitcoin::Transaction =
                bitcoin::consensus::encode::deserialize_hex(signed_tx_hex).unwrap();

            let target_id = session.participants[&participant_id].target_participant;
            let lock_tx_hex = session.lock_txs.get(&target_id).unwrap();
            let lock_tx: bitcoin::Transaction =
                bitcoin::consensus::encode::deserialize_hex(lock_tx_hex).unwrap();
            let prevout = lock_tx.output[0].clone();

            // extract tweaked xonly key from script_pubkey
            let xonly = bitcoin::XOnlyPublicKey::from_slice(&prevout.script_pubkey.as_bytes()[2..])
                .unwrap();

            let mut sighash_cache = bitcoin::sighash::SighashCache::new(&signed_tx);
            let sighash = sighash_cache
                .taproot_key_spend_signature_hash(
                    0,
                    &bitcoin::sighash::Prevouts::All(&[prevout]),
                    bitcoin::sighash::TapSighashType::Default,
                )
                .unwrap();

            let sig_bytes = signed_tx.input[0]
                .witness
                .iter()
                .next()
                .expect("witness should not be empty");
            let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(sig_bytes).unwrap();
            let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());

            bitcoin::secp256k1::Secp256k1::new()
                .verify_schnorr(&sig, &msg, &xonly)
                .unwrap_or_else(|_| {
                    panic!(
                        "daemon {} spend tx signature should verify for {:?}",
                        i + 1,
                        role
                    )
                });

            info!(
                "daemon {} spend tx signature verified for {:?}",
                i + 1,
                role
            );
        }
    }
}
