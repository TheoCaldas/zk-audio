//! circomlib-compatible Poseidon, generic over the prime field.
//!
//! Must agree with the circuits digest for digest, since a verifier
//! recomputes commitments off-circuit and compares them against the
//! proof's terminal IVC state. Instantiate at the scalar field of Nova's
//! primary curve, which is what the circuits compile against. See
//! README.md, "Poseidon parity".
//!
//! Round constants are hex strings decoded into `F` on first use, so one
//! table serves any field. `constants.rs` is generated from
//! `circuits/circom/poseidon_constants.circom`.
//!
//! Reference implementation, not a hot path, and not constant-time.

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
/// Reduces mod the characteristic: circomlib's constants are canonically
/// reduced only for BN254, so a smaller field needs the reduction.
fn decode<F: PrimeField>(hex_be: &str) -> F {
    let bytes = hex::decode(hex_be).expect("generated constant is not valid hex");

    // Horner over bytes; `ff::PrimeField` has no from_be_bytes_mod_order.
    let base = F::from(256u64);
    let mut acc = F::ZERO;
    for b in bytes {
        acc = acc * base + F::from(u64::from(b));
    }
    acc
}

/// Decoded constants for width `t`, built once per `(field, t)`.
///
/// Rust has no generic `static`, so the cache is keyed by `TypeId`. The key
/// pins the field type, so each entry only ever downcasts back to the type
/// it was inserted as.
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
/// Transposed relative to the usual row-times-vector order. Swapping the
/// indices compiles and yields a different digest.
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

/// Mirrors `SpongeBatchHasher(width, rate)` in `hasher_sponge.circom`.
///
/// Capacity-1 chain: one `Poseidon(rate + 1)` per group of `rate`
/// elements, state starting at zero, final group zero-padded. The pad
/// length is fixed by the circuit's compile-time width, so it is not
/// prover-chosen.
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

/// The rate every `*_step.circom` absorbs at.
pub const SPONGE_RATE: usize = 4;

/// One IVC fold: `Poseidon(2)([prev, sponge_batch_hash(batch, 4)])`.
pub fn fold_batch<F: PrimeField>(prev: F, batch: &[F]) -> F {
    poseidon(&[prev, sponge_batch_hash(batch, SPONGE_RATE)])
}

/// The running commitment over a stream, folded from `z0 = 0`.
///
/// `batches` must be the same partition the circuit folds over: one entry
/// per Nova step. The result is what the proof's terminal IVC state holds
/// for that stream.
pub fn running_commitment<'a, F, I>(batches: I) -> F
where
    F: PrimeField,
    I: IntoIterator<Item = &'a [F]>,
{
    batches
        .into_iter()
        .fold(F::ZERO, |acc, batch| fold_batch(acc, batch))
}

/// Parses one packed element as `serialized/*.json` carries it: `"0x<hex>"`,
/// or bare decimal. `None` if it is neither.
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
pub fn from_biguint<F: PrimeField>(v: &num_bigint::BigUint) -> F {
    let base = F::from(256u64);
    let mut acc = F::ZERO;
    for b in v.to_bytes_be() {
        acc = acc * base + F::from(u64::from(b));
    }
    acc
}

/// Renders a field element as `circuit.rs` renders the terminal IVC state:
/// `0x` plus 64 lowercase hex digits. Commitments compare as strings.
pub fn to_hex<F: PrimeField>(f: &F) -> String {
    format!("{f:?}")
}

/// Mirrors `BatchHasher(width)` in `hasher.circom`.
///
/// Chains `Poseidon(2)` from `batch[0]`, not from zero, so a one-element
/// batch hashes to itself. That seeding is why this differs from
/// `sponge_batch_hash` even at rate 1.
///
/// Panics on an empty batch, which `BatchHasher` cannot represent either.
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
/// The two digests differ, so a commitment is only meaningful alongside the
/// construction that produced it. Explicit rather than defaulted, so a
/// caller cannot silently hash a circuit the wrong way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchHashKind {
    /// `SpongeBatchHasher(width, rate)`. Every `*_step.circom`.
    Sponge { rate: usize },
    /// `BatchHasher(width)`. The `_baseline` and `multiplier_*` circuits.
    Chain,
}

impl BatchHashKind {
    /// What the current step circuits use.
    pub const PRODUCTION: Self = Self::Sponge { rate: SPONGE_RATE };

    /// Picks the construction from a circuit name.
    ///
    /// `multiplier_*` circuits hash with the arity-2 chain; everything in
    /// `circuits/edits/<family>/` uses sponge-4. `_baseline` circuits also
    /// use the chain but compile to the same stems, so proving against
    /// those artefacts needs `Chain` passed explicitly.
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
