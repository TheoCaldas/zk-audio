//! Guards the Rust/Circom Poseidon agreement.
//!
//! The Rust reference hash and the circuits once disagreed, because the
//! Rust side ran over BN254 (`light-poseidon`) while the circuits prove
//! over Vesta (`circom --prime vesta`, forced by Nova's Pallas primary
//! curve). A silent recurrence would make the whole protocol
//! unverifiable - a verifier recomputing a commitment would read a
//! mismatch as a forgery - so the agreement is pinned here.
//!
//! The expected values are **not** self-generated. Each was taken from a
//! real circom witness: the circuit was compiled, its witness generated
//! with `node generate_witness.js`, and the output read with
//! `snarkjs wtns export json`. Regenerate the same way if a vector ever
//! needs changing, and never by copying what Rust currently prints.
//!
//! See README.md, "Poseidon parity", for the derivation.

use ff::{Field, PrimeField};
use nova_snark::traits::Engine;
use num_bigint::BigUint;

use zk_audio::poseidon::{self, poseidon as hash, sponge_batch_hash, SPONGE_RATE};

/// The field the circuits prove in: the scalar field of Nova's primary
/// curve, i.e. what `--prime vesta` compiles against.
type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

/// Renders a field element in decimal, the way `snarkjs wtns export json`
/// prints witness values, so expected constants can be pasted in as they
/// were read.
fn dec<F: PrimeField>(f: &F) -> String {
    let hex = poseidon::to_hex(f);
    BigUint::parse_bytes(hex.trim_start_matches("0x").as_bytes(), 16)
        .expect("to_hex produced non-hex")
        .to_string()
}

/* --- THE PRIME IS THE ONE THE CIRCUITS COMPILE AGAINST --- */

/// If this fails, the curve cycle changed and every vector below is
/// meaningless: the circuits would need recompiling under the new prime.
#[test]
fn proving_field_is_vesta() {
    assert_eq!(
        format!("{:?}", Fq::MODULUS),
        "\"0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001\"",
        "Nova's primary scalar field is no longer the Vesta prime that \
         compile.circuit.sh passes as --prime vesta"
    );
}

/* --- PARITY WITH CIRCOM WITNESSES --- */

/// `Poseidon(2)([1,2])`, from a `--prime vesta` witness.
///
/// The BN254 value for the same input is
/// `7853200120776062878684798364095072458815029376092732009249414926327459813530`,
/// which is what `light-poseidon` returned and why the two sides
/// disagreed. Asserting the Vesta value pins the fix.
#[test]
fn poseidon2_matches_circom_vesta() {
    let h = hash::<Fq>(&[Fq::from(1u64), Fq::from(2u64)]);
    assert_eq!(
        dec(&h),
        "10148246943864975455840209516398831844995242484352636702637979101131422116154"
    );
}

/// The same generic code, instantiated at BN254, reproduces the BN254
/// digest.
///
/// This is what shows the port is genuinely field-generic rather than
/// tuned to one field: the constants stay as hex and are decoded into
/// whichever field is asked for, exactly as circom reinterprets them under
/// `--prime`. The expected value is `light-poseidon`'s output.
#[test]
fn poseidon2_matches_bn254_when_instantiated_there() {
    type Bn = <nova_snark::provider::Bn256EngineIPA as Engine>::Scalar;
    let h = hash::<Bn>(&[Bn::from(1u64), Bn::from(2u64)]);
    assert_eq!(
        dec(&h),
        "7853200120776062878684798364095072458815029376092732009249414926327459813530"
    );
}

/// `SpongeBatchHasher(10, 4)` over `[7..16]`, from a `--prime vesta`
/// witness.
///
/// Exercises more than the `Poseidon(2)` vector: arity 5
/// (`Poseidon(rate+1)`, so `t = 6`), the capacity-1 chaining, **and** the
/// zero-padding of the final partial group - 10 elements at rate 4 is two
/// full groups plus a half-empty third.
#[test]
fn sponge4_matches_circom_vesta() {
    let batch: Vec<Fq> = (7u64..17).map(Fq::from).collect();
    let h = sponge_batch_hash(&batch, 4);
    assert_eq!(
        dec(&h),
        "23347766742736862476021340071865967082122274712988910502346682369805372278870"
    );
}

/// `BatchHasher(10)` over `[7..16]`, from a `--prime vesta` witness.
///
/// The legacy `multiplier_*` (`gain2`) family and the `_baseline` circuits
/// hash with this arity-2 chain rather than the sponge, so a commitment
/// computed for them needs this construction. Pinning both digests over
/// the SAME input also documents that they genuinely differ - picking the
/// wrong one yields a valid-looking but wrong commitment.
#[test]
fn chain_batch_hash_matches_circom_vesta() {
    let batch: Vec<Fq> = (7u64..17).map(Fq::from).collect();
    assert_eq!(
        dec(&poseidon::chain_batch_hash(&batch)),
        "9024112491853520307567450724511658984378348853162835341321203510238718083810"
    );

    // Same input, different construction, different digest.
    assert_ne!(
        poseidon::chain_batch_hash(&batch),
        sponge_batch_hash(&batch, 4)
    );
}

/// `BatchHasher` seeds with `batch[0]` rather than a zero capacity, so a
/// single-element batch hashes to itself. The sponge does not.
#[test]
fn chain_seeds_with_the_first_element() {
    let one = [Fq::from(42u64)];
    assert_eq!(poseidon::chain_batch_hash(&one), Fq::from(42u64));
    assert_ne!(sponge_batch_hash(&one, 4), Fq::from(42u64));
}

/// `BatchHashKind` must route each circuit family to the construction its
/// circuit actually uses.
#[test]
fn batch_hash_kind_routes_by_circuit_family() {
    use zk_audio::poseidon::BatchHashKind;

    // The legacy gain2 family predates the sponge switch.
    assert_eq!(
        BatchHashKind::for_circuit("multiplier_b16_B100"),
        BatchHashKind::Chain
    );
    // Current step circuits.
    for name in ["gain_b8_k2_B10", "atten_b8_m1_B100", "tremolo_b8_f5_B10"] {
        assert_eq!(BatchHashKind::for_circuit(name), BatchHashKind::PRODUCTION);
    }

    let batch: Vec<Fq> = (7u64..17).map(Fq::from).collect();
    assert_eq!(
        BatchHashKind::Chain.digest(&batch),
        poseidon::chain_batch_hash(&batch)
    );
    assert_eq!(
        BatchHashKind::PRODUCTION.digest(&batch),
        sponge_batch_hash(&batch, SPONGE_RATE)
    );
}

/* --- STRUCTURAL PROPERTIES OF THE SPONGE --- */

/// A batch that exactly fills its groups must not be padded, and a batch
/// one element short must be - so the two cannot collide.
#[test]
fn sponge_padding_is_not_ambiguous() {
    let full: Vec<Fq> = (1u64..9).map(Fq::from).collect(); // 8 = 2 groups
    let short: Vec<Fq> = (1u64..8).map(Fq::from).collect(); // 7 -> padded

    assert_ne!(
        sponge_batch_hash(&full, 4),
        sponge_batch_hash(&short, 4),
        "a zero-padded batch collided with a full one"
    );

    // Explicitly zero-padding the short batch reproduces the sponge's own
    // padding, which is what makes the pad length unambiguous: it is fixed
    // by the compile-time width, not chosen by the prover.
    let mut padded = short.clone();
    padded.push(Fq::ZERO);
    assert_eq!(sponge_batch_hash(&short, 4), sponge_batch_hash(&padded, 4));
}

/// Changing any single element changes the digest. Cheap, but it catches
/// an absorb loop that drops or overwrites an input.
#[test]
fn sponge_depends_on_every_element() {
    let base: Vec<Fq> = (1u64..=10).map(Fq::from).collect();
    let h0 = sponge_batch_hash(&base, SPONGE_RATE);

    for i in 0..base.len() {
        let mut v = base.clone();
        v[i] += Fq::ONE;
        assert_ne!(
            sponge_batch_hash(&v, SPONGE_RATE),
            h0,
            "digest ignored element {i}"
        );
    }
}

/// The fold is order-sensitive, as an IVC chain must be: two batches
/// swapped between steps give a different running commitment.
#[test]
fn fold_chain_is_order_sensitive() {
    let a: Vec<Fq> = (1u64..=4).map(Fq::from).collect();
    let b: Vec<Fq> = (5u64..=8).map(Fq::from).collect();

    let ab = poseidon::running_commitment([a.as_slice(), b.as_slice()]);
    let ba = poseidon::running_commitment([b.as_slice(), a.as_slice()]);
    assert_ne!(ab, ba, "the fold chain is order-insensitive");
}

/// `running_commitment` must equal the fold applied by hand, starting from
/// `z0 = 0` - the value Nova seeds the IVC state with.
#[test]
fn running_commitment_folds_from_zero() {
    let a: Vec<Fq> = (1u64..=4).map(Fq::from).collect();
    let b: Vec<Fq> = (5u64..=8).map(Fq::from).collect();

    let by_hand = poseidon::fold_batch(poseidon::fold_batch(Fq::ZERO, &a), &b);
    assert_eq!(
        poseidon::running_commitment([a.as_slice(), b.as_slice()]),
        by_hand
    );
}

/* --- THE TEXT FORMATS THE PROTOCOL TRANSPORTS --- */

/// `to_hex` must match how `circuit.rs` renders the terminal IVC state
/// (`format!("{:?}")`), since commitments are compared as strings.
#[test]
fn to_hex_matches_the_debug_rendering_circuit_rs_uses() {
    let f = Fq::from(12345u64);
    assert_eq!(poseidon::to_hex(&f), format!("{f:?}"));
    assert_eq!(
        poseidon::to_hex(&f),
        "0x0000000000000000000000000000000000000000000000000000000000003039"
    );
}

/// Packed elements reach the circuit as `"0x<hex>"` in
/// `serialized/*.json`, and circom input is conventionally decimal, so
/// both must parse - and round-trip.
#[test]
fn parse_field_element_accepts_both_encodings() {
    let hex: Fq = poseidon::parse_field_element("0x3039").expect("hex");
    let dec_: Fq = poseidon::parse_field_element("12345").expect("decimal");
    assert_eq!(hex, Fq::from(12345u64));
    assert_eq!(dec_, Fq::from(12345u64));

    // A real packed chunk, as compress_samples_at writes it.
    let packed = "0x918e908b928a8f8b8c8b8d8b8e8989848a888982898786818184827e847e82";
    let f: Fq = poseidon::parse_field_element(packed).expect("packed chunk");
    let again: Fq = poseidon::parse_field_element(&poseidon::to_hex(&f)).expect("roundtrip");
    assert_eq!(f, again);

    assert!(poseidon::parse_field_element::<Fq>("not a number").is_none());
}
