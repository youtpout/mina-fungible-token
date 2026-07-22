use std::{collections::HashSet, io::Read, str::FromStr, sync::OnceLock};

use ark_ff::{Field, One, PrimeField, Zero};
use flate2::read::GzDecoder;
use mina_curves::pasta::Fp;
use mina_runtime::{CompileCircuitRequest, CompileProgramRequest};
use pickles::recorded::{LinComb, RecordedCircuit, RecordedConstraint};
use poseidon::{full_round, PlonkSpongeConstantsKimchi, SpongeParamsForField};
use serde::Deserialize;

const PROGRAM_BYTES: &[u8] = include_bytes!("../assets/fungible-token-1.1.0.json.gz");
const STRUCTURAL_SEED: &str =
    "3392518251768960475377392625298437850623664973002200885669375116181514017494";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recording {
    circuit: RecordedCircuit,
    witness: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackagedProgram {
    verification_key_hash: String,
    transfer_branch: usize,
    branches: Vec<Recording>,
    transfer_witness_template: Vec<String>,
}

static PROGRAM: OnceLock<PackagedProgram> = OnceLock::new();

fn program() -> Result<&'static PackagedProgram, String> {
    if let Some(program) = PROGRAM.get() {
        return Ok(program);
    }
    let mut decoder = GzDecoder::new(PROGRAM_BYTES);
    let mut json = Vec::new();
    decoder.read_to_end(&mut json).map_err(|error| {
        format!("cannot decompress the embedded FungibleToken program: {error}")
    })?;
    let parsed = serde_json::from_slice(&json)
        .map_err(|error| format!("cannot decode the embedded FungibleToken program: {error}"))?;
    let _ = PROGRAM.set(parsed);
    PROGRAM
        .get()
        .ok_or_else(|| "cannot initialize the embedded FungibleToken program".to_owned())
}

fn parse_field(value: &str) -> Result<Fp, String> {
    Fp::from_str(value).map_err(|_| format!("invalid embedded field element: {value}"))
}

fn parse_witness(values: &[String]) -> Result<Vec<Fp>, String> {
    values.iter().map(|value| parse_field(value)).collect()
}

pub fn verification_key_hash() -> Result<Fp, String> {
    parse_field(&program()?.verification_key_hash)
}

pub fn transfer_branch() -> Result<usize, String> {
    Ok(program()?.transfer_branch)
}

pub fn compile_request() -> Result<CompileProgramRequest, String> {
    let program = program()?;
    Ok(CompileProgramRequest {
        branches: program
            .branches
            .iter()
            .map(|recording| CompileCircuitRequest {
                circuit: recording.circuit.clone(),
                witness: recording.witness.clone(),
                proofs_verified: 0,
            })
            .collect(),
        cache_bytes_base64: None,
        want_cache_bytes: false,
    })
}

fn evaluate(lc: &LinComb, witness: &[Fp], override_value: Option<(u32, Fp)>) -> Fp {
    let mut value = lc.constant.unwrap_or_else(Fp::zero);
    for (coefficient, index) in &lc.terms {
        let term = override_value
            .filter(|(override_index, _)| override_index == index)
            .map(|(_, value)| value)
            .unwrap_or(witness[*index as usize]);
        value += *coefficient * term;
    }
    value
}

fn force_assign(lc: &LinComb, target: Fp, index: u32, witness: &mut [Fp]) -> bool {
    let mut coefficient = Fp::zero();
    let mut known = lc.constant.unwrap_or_else(Fp::zero);
    for (term_coefficient, term_index) in &lc.terms {
        if *term_index == index {
            coefficient += term_coefficient;
        } else {
            known += *term_coefficient * witness[*term_index as usize];
        }
    }
    let Some(inverse) = coefficient.inverse() else {
        return false;
    };
    witness[index as usize] = (target - known) * inverse;
    true
}

fn active_generic_variables(constraint: &RecordedConstraint) -> Vec<u32> {
    let RecordedConstraint::Generic {
        cl,
        l,
        cr,
        r,
        co,
        o,
        m,
        ..
    } = constraint
    else {
        return Vec::new();
    };
    let mut indexes = Vec::new();
    if !cl.is_zero() || !m.is_zero() {
        indexes.extend(l.terms.iter().map(|(_, index)| *index));
    }
    if !cr.is_zero() || !m.is_zero() {
        indexes.extend(r.terms.iter().map(|(_, index)| *index));
    }
    if !co.is_zero() {
        indexes.extend(o.terms.iter().map(|(_, index)| *index));
    }
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

fn generic_value(
    constraint: &RecordedConstraint,
    witness: &[Fp],
    override_value: Option<(u32, Fp)>,
) -> Fp {
    let RecordedConstraint::Generic {
        cl,
        l,
        cr,
        r,
        co,
        o,
        m,
        c,
    } = constraint
    else {
        return Fp::zero();
    };
    let left = evaluate(l, witness, override_value);
    let right = evaluate(r, witness, override_value);
    let output = evaluate(o, witness, override_value);
    *cl * left + *cr * right + *co * output + *m * left * right + c
}

fn singleton_index(lc: &LinComb) -> Option<u32> {
    match (lc.constant, lc.terms.as_slice()) {
        (None, [(coefficient, index)]) if coefficient.is_one() => Some(*index),
        (Some(constant), [(coefficient, index)]) if constant.is_zero() && coefficient.is_one() => {
            Some(*index)
        }
        _ => None,
    }
}

fn seed_is_zero_assertions(
    circuit: &RecordedCircuit,
    witness: &mut [Fp],
    protected: &mut HashSet<u32>,
) {
    for constraints in circuit.constraints.windows(3) {
        let (
            RecordedConstraint::Generic {
                cl,
                l,
                cr,
                r,
                co,
                m,
                c,
                ..
            },
            RecordedConstraint::Generic { o: complement, .. },
            RecordedConstraint::Generic { l: inverse, .. },
        ) = (&constraints[0], &constraints[1], &constraints[2])
        else {
            continue;
        };
        if !m.is_one() || !cl.is_zero() || !cr.is_zero() || !co.is_zero() || !c.is_zero() {
            continue;
        }
        let (Some(selector), Some(value), Some(inverse), Some(result)) = (
            singleton_index(l),
            singleton_index(r),
            singleton_index(inverse),
            singleton_index(complement),
        ) else {
            continue;
        };
        let value = witness[value as usize];
        witness[selector as usize] = if value.is_zero() {
            Fp::one()
        } else {
            Fp::zero()
        };
        witness[result as usize] = if value.is_zero() {
            Fp::zero()
        } else {
            Fp::one()
        };
        witness[inverse as usize] = value.inverse().unwrap_or_else(Fp::zero);
        protected.extend([selector, result, inverse]);
    }
}

fn seed_amount_assertions(amount: Fp, witness: &mut [Fp]) -> Result<(), String> {
    let inverse = amount
        .inverse()
        .ok_or_else(|| "the transfer amount must be non-zero".to_owned())?;
    let minus_one = amount - Fp::one();
    let plus_one = amount + Fp::one();
    witness[404] = Fp::zero();
    witness[405] = inverse;
    witness[407] = Fp::one();
    witness[409] = amount;
    witness[410] = -Fp::one();
    witness[411] = amount;
    witness[412] = Fp::one();
    witness[413] = minus_one;
    witness[414] = minus_one;
    witness[415] = inverse;
    witness[417] = -amount;
    witness[435] = Fp::zero();
    witness[436] = inverse;
    witness[438] = Fp::one();
    witness[440] = amount;
    witness[441] = Fp::one();
    witness[444] = plus_one;
    witness[445] = plus_one;
    witness[446] = (plus_one + Fp::one())
        .inverse()
        .ok_or_else(|| "the transfer amount is outside the supported UInt64 range".to_owned())?;
    witness[448] = amount;
    Ok(())
}

fn repair_poseidon(
    states: &[Vec<LinComb>],
    last: &[LinComb],
    witness: &mut [Fp],
    protected: &HashSet<u32>,
) -> Result<(), String> {
    let (ordered, state) = poseidon_rows(states, witness)?;
    for (recorded, value) in states.iter().skip(1).zip(ordered.iter().skip(1)) {
        for (lc, target) in recorded.iter().zip(value) {
            if let Some(index) = lc
                .terms
                .iter()
                .map(|(_, index)| *index)
                .find(|index| !protected.contains(index))
            {
                let _ = force_assign(lc, *target, index, witness);
            }
        }
    }
    for (lc, target) in last.iter().zip(state) {
        if let Some(index) = lc
            .terms
            .iter()
            .map(|(_, index)| *index)
            .find(|index| !protected.contains(index))
        {
            let _ = force_assign(lc, target, index, witness);
        }
    }
    Ok(())
}

fn poseidon_rows(
    states: &[Vec<LinComb>],
    witness: &[Fp],
) -> Result<(Vec<[Fp; 3]>, [Fp; 3]), String> {
    let initial: [Fp; 3] = states[0]
        .iter()
        .map(|lc| evaluate(lc, witness, None))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "the recorded Poseidon state must have width three".to_owned())?;
    let params = Fp::get_params();
    let mut rows = vec![initial];
    let mut state = initial;
    for round in 0..params.round_constants.len() {
        full_round::<Fp, PlonkSpongeConstantsKimchi>(params, &mut state, round);
        if round + 1 < params.round_constants.len() {
            rows.push(state);
        }
    }
    let mut ordered = Vec::with_capacity(rows.len());
    for chunk in rows.chunks(5) {
        if chunk.len() != 5 {
            return Err("the recorded Poseidon rows are malformed".to_owned());
        }
        ordered.extend([chunk[0], chunk[4], chunk[1], chunk[2], chunk[3]]);
    }
    Ok((ordered, state))
}

fn truncated_field(value: Fp, bits: u32) -> Fp {
    let limbs = value.into_bigint();
    let mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    Fp::from(limbs.as_ref()[0] & mask)
}

fn repair_constraint(
    constraint: &RecordedConstraint,
    witness: &mut [Fp],
    protected: &HashSet<u32>,
) -> Result<bool, String> {
    match constraint {
        RecordedConstraint::Poseidon { states, last } => {
            repair_poseidon(states, last, witness, protected)?;
            Ok(true)
        }
        RecordedConstraint::Endoscalar {
            input,
            output,
            num_bits,
        } => {
            let target = truncated_field(evaluate(input, witness, None), *num_bits);
            let index = output
                .terms
                .iter()
                .map(|(_, index)| *index)
                .find(|index| !protected.contains(index));
            Ok(index.is_some_and(|index| force_assign(output, target, index, witness)))
        }
        RecordedConstraint::Equal { l, r } => {
            if evaluate(l, witness, None) == evaluate(r, witness, None) {
                return Ok(false);
            }
            let candidates = r
                .terms
                .iter()
                .chain(&l.terms)
                .map(|(_, index)| *index)
                .filter(|index| !protected.contains(index));
            for index in candidates {
                let l0 = evaluate(l, witness, Some((index, Fp::zero())));
                let l1 = evaluate(l, witness, Some((index, Fp::one())));
                let r0 = evaluate(r, witness, Some((index, Fp::zero())));
                let r1 = evaluate(r, witness, Some((index, Fp::one())));
                let linear = l1 - l0 - (r1 - r0);
                if let Some(inverse) = linear.inverse() {
                    witness[index as usize] = -(l0 - r0) * inverse;
                    return Ok(true);
                }
            }
            Ok(false)
        }
        RecordedConstraint::Generic { co, o, .. } => {
            if generic_value(constraint, witness, None).is_zero() {
                return Ok(false);
            }
            let mut candidates = Vec::new();
            if !co.is_zero() {
                candidates.extend(o.terms.iter().map(|(_, index)| *index));
            }
            candidates.extend(active_generic_variables(constraint));
            let mut seen = HashSet::new();
            for index in candidates {
                if protected.contains(&index) || !seen.insert(index) {
                    continue;
                }
                let f0 = generic_value(constraint, witness, Some((index, Fp::zero())));
                let f1 = generic_value(constraint, witness, Some((index, Fp::one())));
                let f2 = generic_value(constraint, witness, Some((index, Fp::from(2u64))));
                let quadratic = (f2 - f1 - f1 + f0) * Fp::from(2u64).inverse().unwrap();
                let linear = f1 - f0 - quadratic;
                if quadratic.is_zero() {
                    if let Some(inverse) = linear.inverse() {
                        witness[index as usize] = -f0 * inverse;
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        other => Err(format!(
            "unsupported transfer witness constraint: {other:?}"
        )),
    }
}

fn validate_witness(circuit: &RecordedCircuit, witness: &[Fp]) -> Result<(), String> {
    for (index, constraint) in circuit.constraints.iter().enumerate() {
        let valid = match constraint {
            RecordedConstraint::Generic { .. } => {
                generic_value(constraint, witness, None).is_zero()
            }
            RecordedConstraint::Equal { l, r } => {
                evaluate(l, witness, None) == evaluate(r, witness, None)
            }
            RecordedConstraint::Endoscalar {
                input,
                output,
                num_bits,
            } => {
                evaluate(output, witness, None)
                    == truncated_field(evaluate(input, witness, None), *num_bits)
            }
            RecordedConstraint::Poseidon { states, last } => {
                let (ordered, final_state) = poseidon_rows(states, witness)?;
                states
                    .iter()
                    .skip(1)
                    .zip(ordered.iter().skip(1))
                    .all(|(recorded, expected)| {
                        recorded
                            .iter()
                            .zip(expected)
                            .all(|(lc, value)| evaluate(lc, witness, None) == *value)
                    })
                    && last
                        .iter()
                        .zip(final_state)
                        .all(|(lc, value)| evaluate(lc, witness, None) == value)
            }
            other => {
                return Err(format!(
                    "unsupported transfer witness constraint: {other:?}"
                ))
            }
        };
        if !valid {
            return Err(format!(
                "generated transfer witness fails constraint {index}: {constraint:?}"
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct TransferWitnessInput {
    pub account_update_hash: Fp,
    pub calls_hash: Fp,
    pub token_x: Fp,
    pub token_is_odd: bool,
    pub sender_x: Fp,
    pub sender_is_odd: bool,
    pub receiver_x: Fp,
    pub receiver_is_odd: bool,
    pub amount: u64,
    pub blinding: Fp,
}

pub fn generate_transfer_witness(input: TransferWitnessInput) -> Result<Vec<String>, String> {
    let program = program()?;
    let circuit = &program
        .branches
        .get(program.transfer_branch)
        .ok_or_else(|| "the embedded transfer branch is missing".to_owned())?
        .circuit;
    let mut witness = parse_witness(&program.transfer_witness_template)?;
    let mut protected: HashSet<u32> = [0, 1, 2, 3, 5, 6, 7, 9, 10, 12, 25, 1109]
        .into_iter()
        .collect();
    witness[0] = input.account_update_hash;
    witness[1] = input.calls_hash;
    witness[2] = input.token_x;
    witness[3] = Fp::from(input.token_is_odd as u64);
    witness[5] = Fp::one();
    witness[6] = input.sender_x;
    witness[7] = Fp::from(input.sender_is_odd as u64);
    witness[9] = input.receiver_x;
    witness[10] = Fp::from(input.receiver_is_odd as u64);
    witness[12] = Fp::from(input.amount);
    witness[25] = input.blinding;
    witness[1109] = parse_field(STRUCTURAL_SEED)?;
    seed_amount_assertions(witness[12], &mut witness)?;
    seed_is_zero_assertions(circuit, &mut witness, &mut protected);

    for _ in 0..20 {
        seed_amount_assertions(witness[12], &mut witness)?;
        seed_is_zero_assertions(circuit, &mut witness, &mut protected);
        for constraint in &circuit.constraints {
            seed_is_zero_assertions(circuit, &mut witness, &mut protected);
            let _ = repair_constraint(constraint, &mut witness, &protected)?;
        }
    }
    validate_witness(circuit, &witness)?;
    Ok(witness.into_iter().map(|value| value.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regenerates_the_embedded_o1js_transfer_witness() {
        let program = program().expect("embedded program");
        let expected = parse_witness(&program.transfer_witness_template).expect("template witness");
        let generated = generate_transfer_witness(TransferWitnessInput {
            account_update_hash: expected[0],
            calls_hash: expected[1],
            token_x: expected[2],
            token_is_odd: expected[3].is_one(),
            sender_x: expected[6],
            sender_is_odd: expected[7].is_one(),
            receiver_x: expected[9],
            receiver_is_odd: expected[10].is_one(),
            amount: 1_234_567_890,
            blinding: expected[25],
        })
        .expect("generated witness");
        assert_eq!(generated, program.transfer_witness_template);
    }

    #[test]
    fn embeds_the_o1js_2_15_verification_key() {
        assert_eq!(
            verification_key_hash()
                .expect("verification key")
                .to_string(),
            "11275266297357989434659649579180929660472107786900344600948115953037388411671"
        );
        assert_eq!(transfer_branch().expect("transfer branch"), 7);
    }

    #[test]
    fn regenerates_a_distinct_o1js_transfer_vector() {
        let field = |value: &str| parse_field(value).expect("field test vector");
        let generated = generate_transfer_witness(TransferWitnessInput {
            account_update_hash: field(
                "11909561140019905098978899476582907211622221136408647825565176977949056361901",
            ),
            calls_hash: field(
                "7652688051181415380811715514734574835653779492253782967014461790261901546813",
            ),
            token_x: field(
                "2919996120512407313014062828808255422013969845275374011406132452783934981066",
            ),
            token_is_odd: false,
            sender_x: field(
                "26128929354271999245285962662286734919718711533760999607904852494858390193731",
            ),
            sender_is_odd: false,
            receiver_x: field(
                "28755616151314178074317148383765615429855032205626421959078684078997276329907",
            ),
            receiver_is_odd: true,
            amount: 777,
            blinding: field(
                "8554297942514439850942263858623548274610807464913380485764394084910558078826",
            ),
        })
        .expect("generated witness");
        assert_eq!(
            generated[9046],
            "9260899708584619633340573023080427628423003437019317592366002278244325331248"
        );
        assert_eq!(
            generated[9376],
            "7652688051181415380811715514734574835653779492253782967014461790261901546813"
        );
        assert_eq!(
            generated[13179],
            "7943030595517756979243385114672728906412157574924148744111853466441205051544"
        );
    }
}
