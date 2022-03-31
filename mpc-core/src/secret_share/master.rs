//! 2-Party Elliptic curve secret-sharing using Paillier Cryptosystem

use super::slave::{S1, S2, S3};
use super::{SecretShare, P};
use curv::arithmetic::{Converter, Modulo};
use p256::EncodedPoint;
use paillier::*;

pub struct Initialized {
    /// X coordinate of master's secret
    x: BigInt,
    /// Y coordinate of master's secret
    y: BigInt,
}
pub struct StepOne;
pub struct StepTwo {
    /// A * M_A mod p
    a_masked_mod_p: BigInt,
}
pub struct StepThree;
pub struct Complete {
    /// Master's secret
    secret: BigInt,
}

pub trait State {}
impl State for Initialized {}
impl State for StepOne {}
impl State for StepTwo {}
impl State for StepThree {}
impl State for Complete {}

pub struct SecretShareMasterCore<S>
where
    S: State,
{
    /// NIST P-256 Prime
    p: BigInt,
    /// Current state of secret share protocol
    state: S,
    /// Master's Paillier encryption key
    enc_key: EncryptionKey,
    /// Master's Paillier decryption key
    dec_key: DecryptionKey,
}

#[derive(Debug)]
pub struct M1 {
    /// Master's encryption key
    pub(crate) enc_key: EncryptionKey,
    /// E(x_q)
    pub(crate) e_x_q: BigInt,
    /// E(-x_q)
    pub(crate) e_neg_x_q: BigInt,
    /// E(y_q^2)
    pub(crate) e_y_q_pow_2: BigInt,
    /// E(-2y_q)
    pub(crate) e_neg_2_y_q: BigInt,
}

#[derive(Debug)]
pub struct M2 {
    /// E((T * M_T)^p-3 mod p)
    pub(crate) e_t_mod_pow: BigInt,
}

#[derive(Debug)]
pub struct M3 {
    /// E(A * M_A * B * M_B)
    pub(crate) e_ab_masked: BigInt,
}

impl SecretShareMasterCore<Initialized> {
    pub fn new(point: &EncodedPoint) -> Self {
        let (enc_key, dec_key) = Paillier::keypair().keys();
        Self {
            state: Initialized {
                x: BigInt::from_bytes(point.x().expect("Invalid point")),
                y: BigInt::from_bytes(point.y().expect("Invalid point, or compressed")),
            },
            p: BigInt::from_hex(P).unwrap(),
            enc_key,
            dec_key,
        }
    }

    pub fn next(self) -> (M1, SecretShareMasterCore<StepOne>) {
        // Computes E(x_q)
        let e_x_q: BigInt =
            Paillier::encrypt(&self.enc_key, RawPlaintext::from(&self.state.x)).into();

        // Computes E(-x_q)
        let e_neg_x_q: BigInt = Paillier::encrypt(
            &self.enc_key,
            RawPlaintext::from(BigInt::mod_sub(&self.p, &self.state.x, &self.p)),
        )
        .into();

        // Computes E(y_q^2)
        let e_y_q_pow_2: BigInt = Paillier::encrypt(
            &self.enc_key,
            RawPlaintext::from(BigInt::mod_pow(
                &self.state.y,
                &BigInt::from(2_u16),
                &self.p,
            )),
        )
        .into();

        // Computes E(-2y_q)
        let e_neg_2_y_q: BigInt = Paillier::encrypt(
            &self.enc_key,
            RawPlaintext::from(BigInt::mod_sub(&self.p, &(2 * &self.state.y), &self.p)),
        )
        .into();

        (
            M1 {
                enc_key: self.enc_key.clone(),
                e_x_q,
                e_neg_x_q,
                e_y_q_pow_2,
                e_neg_2_y_q,
            },
            SecretShareMasterCore {
                state: StepOne,
                enc_key: self.enc_key,
                dec_key: self.dec_key,
                p: self.p,
            },
        )
    }
}

impl SecretShareMasterCore<StepOne> {
    pub fn next(self, s: S1) -> (M2, SecretShareMasterCore<StepTwo>) {
        // Computes A * M_A mod p
        let a_masked: BigInt =
            Paillier::decrypt(&self.dec_key, RawCiphertext::from(s.e_a_masked)).into();
        let a_masked_mod_p = BigInt::mod_sub(&a_masked, &s.n_a_mod_p, &self.p);

        // Computes T * M_T mod p
        let t_masked: BigInt =
            Paillier::decrypt(&self.dec_key, RawCiphertext::from(s.e_t_masked)).into();
        let t_masked_mod_p = BigInt::mod_sub(&t_masked, &s.n_t_mod_p, &self.p);

        // Computes E((T * M_T)^p-3 mod p)
        let t_mod_pow = BigInt::mod_pow(&t_masked_mod_p, &(&self.p - 3), &self.p);
        let e_t_mod_pow: BigInt =
            Paillier::encrypt(&self.enc_key, RawPlaintext::from(t_mod_pow)).into();

        (
            M2 { e_t_mod_pow },
            SecretShareMasterCore {
                state: StepTwo { a_masked_mod_p },
                enc_key: self.enc_key,
                dec_key: self.dec_key,
                p: self.p,
            },
        )
    }
}

impl SecretShareMasterCore<StepTwo> {
    pub fn next(self, s: S2) -> (M3, SecretShareMasterCore<StepThree>) {
        // Computes B * M_B mod p
        let b_masked: BigInt =
            Paillier::decrypt(&self.dec_key, RawCiphertext::from(s.e_b_masked)).into();
        let b_masked_mod_p = BigInt::mod_sub(&b_masked, &s.n_b_mod_p, &self.p);

        // Computes E(A * M_A * B * M_B)
        let e_ab_masked: BigInt = Paillier::encrypt(
            &self.enc_key,
            RawPlaintext::from(BigInt::mod_mul(
                &b_masked_mod_p,
                &self.state.a_masked_mod_p,
                &self.p,
            )),
        )
        .into();

        (
            M3 { e_ab_masked },
            SecretShareMasterCore {
                state: StepThree,
                enc_key: self.enc_key,
                dec_key: self.dec_key,
                p: self.p,
            },
        )
    }
}

impl SecretShareMasterCore<StepThree> {
    pub fn next(self, s: S3) -> SecretShareMasterCore<Complete> {
        // Computes master's secret, s_p
        let pms_masked: BigInt =
            Paillier::decrypt(&self.dec_key, RawCiphertext::from(s.e_pms_masked)).into();

        SecretShareMasterCore {
            state: Complete {
                secret: pms_masked % &self.p,
            },
            enc_key: self.enc_key,
            dec_key: self.dec_key,
            p: self.p,
        }
    }
}

impl SecretShareMasterCore<Complete> {
    pub fn secret(self) -> SecretShare {
        self.state.secret
    }
}
