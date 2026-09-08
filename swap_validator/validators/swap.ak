use aiken/builtin
use aiken/interval.{Finite}
use cardano/transaction.{OutputReference, Transaction}

pub type SwapDatum {
  aggregate_pubkey: ByteArray,
  /// Refund deadline as POSIX time in milliseconds, not a slot number:
  /// `validity_range` is POSIX ms, so a slot here makes the guard below vacuous.
  refund_posix_ms: Int,
}

pub type SwapRedeemer {
  Spend { signature: ByteArray }
  Refund { signature: ByteArray }
}

validator swap {
  spend(
    datum: Option<SwapDatum>,
    redeemer: SwapRedeemer,
    utxo: OutputReference,
    self: Transaction,
  ) {
    expect Some(d) = datum
    when redeemer is {
      Spend { signature } ->
        builtin.verify_schnorr_secp256k1_signature(
          d.aggregate_pubkey,
          utxo.transaction_id,
          signature,
        )
      Refund { signature } -> {
        expect Finite(lower) = self.validity_range.lower_bound.bound_type
        expect lower >= d.refund_posix_ms
        let refund_msg =
          builtin.blake2b_256(
            builtin.cons_bytearray(1, utxo.transaction_id),
          )
        builtin.verify_schnorr_secp256k1_signature(
          d.aggregate_pubkey,
          refund_msg,
          signature,
        )
      }
    }
  }
}

// BIP-340 test vector from the spec (should always pass)
test bip340_test_vector() {
  let pubkey =
    #"F9308A019258C31049344F85F89D5229B531C845836F99B08601F113BCE036F9"
  let msg =
    #"0000000000000000000000000000000000000000000000000000000000000000"
  let sig =
    #"E907831F80848D1069A5371B402410364BDF1C5F8307B0084C55F1CE2DCA821525F66A4A85EA8B71E482A74F382D2CE5EBEEE8FDB2172F477DF4900D310536C0"
  builtin.verify_schnorr_secp256k1_signature(pubkey, msg, sig)
}
