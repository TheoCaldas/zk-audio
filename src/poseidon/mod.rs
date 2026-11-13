//! circomlib-compatible Poseidon, generic over the prime field.
//!
//! # Why this module exists
//!
//! The Rust reference hash and the Circom circuits must agree digest for
//! digest, or none of the end-to-end protocol works: the verifier
//! recomputes a commitment off-circuit and compares it against the
//! proof's terminal IVC state, so a disagreement is indistinguishable
//! from a forgery.
//!
//! They did not agree, and the reason was the **field**, not the
//! construction:
//!
//! | side | field | `Poseidon(2)([1,2])` |
//! |---|---|---|
//! | `light-poseidon` + `ark_bn254::Fr` | BN254 scalar | `7853200120776062878684798364095072458815029376092732009249414926327459813530` |
//! | circom `--prime vesta` | Vesta base | `10148246943864975455840209516398831844995242484352636702637979101131422116154` |
//!
//! The circuits are compiled `--prime vesta` because `circuit.rs` folds
//! with `PallasEngine` as the primary curve, and a Nova step circuit lives
//! over the primary curve's *scalar* field - which is exactly
//! `halo2curves::pasta::fq::Fq`, modulus
//! `0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001`.
//! That is the Vesta prime. So the field choice is forced by the curve
//! cycle, and it was the BN254 Rust helper that was wrong.
//!
//! No crate fixes this. `light-poseidon` is hard-wired to BN254
//! parameters, and `neptune` is absent from the tree at `nova-snark` 0.76
//! and implements a different construction that would not match even on
//! the right field. Hence this port.
//!
//! # Why the port matches
//!
//! circomlib's Poseidon is a parameterised permutation whose round
//! constants are **literal hex** in `poseidon_constants.circom`. Circom
//! reinterprets those literals modulo whatever prime it compiles against;
//! the structure around them - `Ark`, `Sigma` (x^5), `Mix`/`MixS`/
//! `MixLast`, `nRoundsF = 8`, the `N_ROUNDS_P` table, `initialState = 0` -
//! is field-agnostic. Read the same constants, run the same rounds, change
//! only the modulus, and the digest follows. So this is generic over
//! `ff::PrimeField` and instantiating it at a different field reproduces
//! that field's circom output, BN254 included.
//!
//! Verified against real circom witnesses; see `tests/poseidon_parity.rs`.
//!
//! # Scope
//!
//! This is a reference implementation for the off-circuit side of the
//! protocol, not a hot path, and it is not constant-time. Do not use it to
//! hash secrets where timing is observable.

pub mod constants;

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use ff::PrimeField;

use constants::{MAX_T, MIN_T, N_ROUNDS_F, N_ROUNDS_P, POSEIDON_C, POSEIDON_M, POSEIDON_P, POSEIDON_S};

/// Decoded round constants for one width `t`, over one field.
struct Params<F> {
    /// `t * N_ROUNDS_F + N_ROUNDS_P[t-2]` round constants.
    c: Vec<F>,
    /// `N_ROUNDS_P[t-2] * (2t-1)` sparse-matrix entries.
    s: Vec<F>,
    /// MDS matrix, `t * t`, row-major.
    m: Vec<F>,
    /// Pre-sparsified MDS matrix, `t * t`, row-major.
    p: Vec<F>,
}

/// Decodes one big-endian hex constant into `F`.
///
/// This is the step that makes the table field-agnostic: the same string
/// becomes a different field element depending on `F`, mirroring how
/// circom reinterprets the literal under `--prime`.
///
/// Reduces modulo the field characteristic rather than requiring the value
/// to already be in range, because a circomlib constant is only
/// canonically reduced for the field it was generated over (BN254). Under
/// a larger prime such as Vesta every constant happens to be in range, but
/// relying on that would make the generic claim false for any field
/// smaller than BN254.
fn decode<F: PrimeField>(hex_be: &str) -> F {
    let bytes = hex::decode(hex_be).expect("generated constant is not valid hex");

    // Horner over bytes: val = val * 256 + byte. Avoids needing a
    // from_be_bytes_mod_order, which `ff::PrimeField` does not provide.
    let base = F::from(256u64);
    let mut acc = F::ZERO;
    for b in bytes {
        acc = acc * base + F::from(u64::from(b));
    }
    acc
}

/// Per-field, per-width constant cache.
///
/// Decoding one width is a few hundred to a few thousand field
/// conversions, and a running commitment calls the permutation once per
/// batch, so the tables are decoded on first use and shared thereafter.
///
/// Rust has no generic `static`, so the cache is a type-erased map keyed
/// by the concrete field type. Values are `Arc<Params<F>>` boxed as
/// `dyn Any + Send + Sync`; the key guarantees each entry is only ever
/// downcast back to the type it was inserted as, so the downcast cannot
/// fail and no `unsafe` is needed.
fn params<F: PrimeField>(t: usize) -> Arc<Params<F>> {
    assert!(
        (MIN_T..=MAX_T).contains(&t),
        "no circomlib Poseidon constants for t = {t} (have {MIN_T}..={MAX_T})"
    );

    type Cache = HashMap<(TypeId, usize), Box<dyn Any + Send + Sync>>;
    static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    let key = (TypeId::of::<F>(), t);

    if let Some(hit) = cache
        .read()
        .expect("Poseidon params cache poisoned")
        .get(&key)
    {
        return hit
            .downcast_ref::<Arc<Params<F>>>()
            .expect("cache entry downcast: key includes the field TypeId")
            .clone();
    }

    // Build outside the write lock; a lost race just discards this copy.
    let built = Arc::new(build_params::<F>(t));

    let mut w = cache.write().expect("Poseidon params cache poisoned");
    let entry = w
        .entry(key)
        .or_insert_with(|| Box::new(Arc::clone(&built)));
    entry
        .downcast_ref::<Arc<Params<F>>>()
        .expect("cache entry downcast: key includes the field TypeId")
        .clone()
}

fn build_params<F: PrimeField>(t: usize) -> Params<F> {
    let i = t - MIN_T;
    let dec = |table: &[&str]| table.iter().map(|h| decode::<F>(h)).collect::<Vec<F>>();
    Params {
        c: dec(POSEIDON_C[i]),
        s: dec(POSEIDON_S[i]),
        m: dec(POSEIDON_M[i]),
        p: dec(POSEIDON_P[i]),
    }
}

/// circomlib's `Sigma`: the S-box, `x^5`.
#[inline]
fn sigma<F: PrimeField>(x: F) -> F {
    let x2 = x * x;
    let x4 = x2 * x2;
    x4 * x
}

/// circomlib's `Mix`: `out[i] = sum_j mat[j][i] * state[j]`.
///
/// Note the index order - the matrix is applied **transposed** relative to
/// the usual `row * vector` convention. This follows `Mix` in
/// `poseidon.circom` literally; swapping it silently yields a different
/// digest, which is precisely the kind of divergence this port exists to
/// avoid.
fn mix<F: PrimeField>(state: &[F], mat: &[F], t: usize) -> Vec<F> {
    (0..t)
        .map(|i| {
            let mut acc = F::ZERO;
            for j in 0..t {
                acc += mat[j * t + i] * state[j];
            }
            acc
        })
        .collect()
}

/// The circomlib Poseidon hash of `inputs`, with `initialState = 0`.
///
/// This is `Poseidon(inputs.len())` in circom: state width is
/// `t = inputs.len() + 1`, the capacity element starts at zero, and the
/// digest is the first element of the final state.
///
/// Panics if `inputs` is empty or wider than the available constants
/// (`Poseidon(2)`..`Poseidon(16)`).
pub fn poseidon<F: PrimeField>(inputs: &[F]) -> F {
    poseidon_with_state(inputs, F::ZERO)
}

/// `PoseidonEx` with an explicit `initialState`, returning one output.
///
/// The circuits here only ever use `initialState = 0` (via `Poseidon`),
/// but the parameter is what distinguishes the compression function from
/// the hash, so it is exposed for the sponge in [`sponge_batch_hash`] to
/// be expressible either way.
pub fn poseidon_with_state<F: PrimeField>(inputs: &[F], initial_state: F) -> F {
    let n = inputs.len();
    assert!(n > 0, "Poseidon needs at least one input");
    let t = n + 1;
    let n_rounds_p = N_ROUNDS_P[t - 2];
    let pa = params::<F>(t);
    let (c, s, m, p) = (&pa.c, &pa.s, &pa.m, &pa.p);

    // ark[0]: state is (initialState, inputs...) plus the first constants.
    let mut state: Vec<F> = Vec::with_capacity(t);
    state.push(initial_state + c[0]);
    for (j, &x) in inputs.iter().enumerate() {
        state.push(x + c[j + 1]);
    }

    // First half of the full rounds: S-box on every element, add
    // constants, mix with M.
    for r in 0..(N_ROUNDS_F / 2 - 1) {
        for x in state.iter_mut() {
            *x = sigma(*x);
        }
        for (j, x) in state.iter_mut().enumerate() {
            *x += c[(r + 1) * t + j];
        }
        state = mix(&state, m, t);
    }

    // Transition into the partial rounds: the last full round mixes with
    // the pre-sparsified P instead of M.
    for x in state.iter_mut() {
        *x = sigma(*x);
    }
    for (j, x) in state.iter_mut().enumerate() {
        *x += c[(N_ROUNDS_F / 2) * t + j];
    }
    state = mix(&state, p, t);

    // Partial rounds: S-box on element 0 only, one constant, sparse mix.
    for r in 0..n_rounds_p {
        state[0] = sigma(state[0]) + c[(N_ROUNDS_F / 2 + 1) * t + r];

        // circomlib's MixS. new[0] folds the whole state; the rest take a
        // multiple of the (already updated) element 0, so element 0 must
        // be read before it is overwritten.
        let row = (t * 2 - 1) * r;
        let mut new0 = F::ZERO;
        for i in 0..t {
            new0 += s[row + i] * state[i];
        }
        let head = state[0];
        for i in 1..t {
            state[i] += head * s[row + t + i - 1];
        }
        state[0] = new0;
    }

    // Second half of the full rounds. Constants continue after the block
    // the partial rounds consumed.
    let off = (N_ROUNDS_F / 2 + 1) * t + n_rounds_p;
    for r in 0..(N_ROUNDS_F / 2 - 1) {
        for x in state.iter_mut() {
            *x = sigma(*x);
        }
        for (j, x) in state.iter_mut().enumerate() {
            *x += c[off + r * t + j];
        }
        state = mix(&state, m, t);
    }

    // MixLast with s = 0: only the first output column is needed.
    for x in state.iter_mut() {
        *x = sigma(*x);
    }
    let mut out = F::ZERO;
    for j in 0..t {
        out += m[j * t] * state[j];
    }
    out
}

/// Sponge-mode batch digest: the Rust mirror of `SpongeBatchHasher(width,
/// rate)` in `circuits/hasher_sponge.circom`.
///
/// Capacity-1 chain: each permutation absorbs `rate` elements alongside the
/// running state, `Poseidon(rate + 1)` per group, state starting at zero.
/// The final partial group is zero-padded, which is unambiguous because
/// `width` and `rate` are compile-time constants in the circuit (so the pad
/// length is fixed, not prover-chosen).
///
/// As the circuit's own note records, this is Merkle-Damgard over
/// circomlib's fixed-length Poseidon rather than the spec sponge, and its
/// digest deliberately differs from `BatchHasher`'s arity-2 chain. Matching
/// the circuit is the whole point, so the construction is copied exactly
/// rather than improved.
pub fn sponge_batch_hash<F: PrimeField>(batch: &[F], rate: usize) -> F {
    assert!(rate > 0, "sponge rate must be positive");

    let nperm = batch.len().div_ceil(rate);
    let mut state = F::ZERO;

    let mut inputs = Vec::with_capacity(rate + 1);
    for i in 0..nperm {
        inputs.clear();
        inputs.push(state);
        for j in 0..rate {
            let idx = i * rate + j;
            inputs.push(batch.get(idx).copied().unwrap_or(F::ZERO));
        }
        state = poseidon(&inputs);
    }
    state
}

/// The rate the production step circuits absorb at: every current
/// `*_step.circom` uses `SpongeBatchHasher(batch_size, 4)`.
pub const SPONGE_RATE: usize = 4;

/// One IVC fold of the running commitment, mirroring the step circuits:
///
/// ```text
/// step_out = Poseidon(2)([step_in, SpongeBatchHasher(batch, 4)])
/// ```
///
/// This is the `Hasher(2)` that every step circuit applies to
/// `(prev_hash, batch_digest)`.
pub fn fold_batch<F: PrimeField>(prev: F, batch: &[F]) -> F {
    poseidon(&[prev, sponge_batch_hash(batch, SPONGE_RATE)])
}

/// The running commitment over a whole stream, starting from `z0 = 0`.
///
/// `batches` must be the SAME partition of the stream the circuit folds
/// over - one entry per Nova step, each of `batch_size` packed field
/// elements. The result is what the proof's terminal IVC state should
/// hold for that stream, so a verifier compares this against
/// `final_hash_original` / `final_hash_edited`.
///
/// Nova is seeded with `z0 = 0` for the hash elements, matching the
/// circuit's `step_in` on the first fold.
pub fn running_commitment<'a, F, I>(batches: I) -> F
where
    F: PrimeField,
    I: IntoIterator<Item = &'a [F]>,
{
    batches
        .into_iter()
        .fold(F::ZERO, |acc, batch| fold_batch(acc, batch))
}

/* --- INTEROP WITH THE SERIALIZED CIRCUIT INPUTS --- */

/// Parses one packed element as the circuit's JSON carries it.
///
/// `serialized/*.json` stores each packed chunk as `"0x<hex>"` (see
/// `audio_parser::compress_samples_at`), so a verifier that recomputes a
/// commitment from those files needs exactly this conversion. Also accepts
/// bare decimal, which is how circom input JSON is usually written.
///
/// Returns `None` if the text parses as neither.
pub fn parse_field_element<F: PrimeField>(s: &str) -> Option<F> {
    let t = s.trim();
    let digits = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"));
    let value = match digits {
        Some(h) => num_bigint::BigUint::parse_bytes(h.as_bytes(), 16)?,
        None => num_bigint::BigUint::parse_bytes(t.as_bytes(), 10)?,
    };
    Some(from_biguint(&value))
}

/// Reduces a big unsigned integer into `F`.
///
/// Packed chunks are up to `256 - bit_depth` bits wide, deliberately kept
/// under the field size by `comp_size_for`, so in practice no reduction
/// occurs - but going through the modulus keeps the conversion total.
pub fn from_biguint<F: PrimeField>(v: &num_bigint::BigUint) -> F {
    let base = F::from(256u64);
    let mut acc = F::ZERO;
    for b in v.to_bytes_be() {
        acc = acc * base + F::from(u64::from(b));
    }
    acc
}

/// Renders a field element the way `circuit.rs` renders the terminal IVC
/// state, i.e. `format!("{:?}")`: `0x` followed by 64 lowercase hex
/// digits.
///
/// Having this here means a recomputed commitment can be compared against
/// `ProofMetrics::final_hash_original` / `final_hash_edited` as strings,
/// without the caller depending on how `Debug` happens to be implemented.
pub fn to_hex<F: PrimeField>(f: &F) -> String {
    format!("{f:?}")
}

/// The arity-2 chained batch digest: the Rust mirror of `BatchHasher(width)`
/// in `circuits/hasher.circom`.
///
/// Merkle-Damgard over `Poseidon(2)`, seeded with the batch's FIRST element
/// rather than zero:
///
/// ```text
/// h = batch[0]
/// for i in 1..width:  h = Poseidon(2)([h, batch[i]])
/// ```
///
/// That seeding is not an implementation detail - it is why this digest
/// differs from `sponge_batch_hash`'s even at rate 1, and why a batch of
/// one element hashes to itself unchanged.
///
/// Needed because the two circuit generations hash differently: the
/// `_baseline` circuits and the legacy `gain2`/`multiplier_*` family use
/// this chain, while every current `*_step.circom` uses the rate-4 sponge.
/// A commitment computed with the wrong one is simply a different hash, so
/// the choice has to follow the circuit - see [`BatchHashKind`].
///
/// Panics on an empty batch, which `BatchHasher` cannot represent either
/// (it instantiates `width - 1` hashers).
pub fn chain_batch_hash<F: PrimeField>(batch: &[F]) -> F {
    assert!(!batch.is_empty(), "BatchHasher needs at least one element");
    let mut h = batch[0];
    for &x in &batch[1..] {
        h = poseidon(&[h, x]);
    }
    h
}

/// Which batch-digest construction a circuit uses.
///
/// The repo ships two generations whose digests differ, so a commitment is
/// only meaningful alongside the construction that produced it. Making this
/// an explicit parameter rather than a default avoids silently computing
/// the wrong hash for a circuit - the failure mode that motivated
/// README.md's "two batch-digest constructions" note in the first place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchHashKind {
    /// `SpongeBatchHasher(width, rate)`. Every current `*_step.circom`.
    Sponge { rate: usize },
    /// `BatchHasher(width)`, the arity-2 chain. The `_baseline` circuits
    /// and the legacy `multiplier_*` (`gain2`) family.
    Chain,
}

impl BatchHashKind {
    /// The construction the current production step circuits use.
    pub const PRODUCTION: Self = Self::Sponge { rate: SPONGE_RATE };

    /// Picks the construction from a circuit name as
    /// `bench::config::circuit_name_for` builds it.
    ///
    /// The `multiplier_*` stems are the legacy `gain2` family, which
    /// predates the sponge switch and still hashes with the arity-2 chain.
    /// Everything else in `circuits/edits/<family>/` is sponge-4.
    /// `_baseline` circuits also use the chain, but they are compiled to
    /// the same stems, so a caller proving against `artifacts/` built from
    /// `circuits/edits/_baseline/` must say so explicitly.
    pub fn for_circuit(circuit_name: &str) -> Self {
        if circuit_name.starts_with("multiplier") {
            Self::Chain
        } else {
            Self::PRODUCTION
        }
    }

    /// Applies the construction to one batch.
    pub fn digest<F: PrimeField>(self, batch: &[F]) -> F {
        match self {
            Self::Sponge { rate } => sponge_batch_hash(batch, rate),
            Self::Chain => chain_batch_hash(batch),
        }
    }
}

/// One IVC fold using an explicit batch-digest construction.
pub fn fold_batch_with<F: PrimeField>(prev: F, batch: &[F], kind: BatchHashKind) -> F {
    poseidon(&[prev, kind.digest(batch)])
}
