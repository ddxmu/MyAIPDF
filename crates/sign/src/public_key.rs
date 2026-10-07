//! PDF certificate encryption: CMS EnvelopedData (RSA key transport, AES-256-CBC).
//! ISO 32000-2 §7.6.5; RFC 5652. This is encryption, not certificate trust validation.

use crate::{
    Certificate, DigitalId, PublicKey, SignError,
    der::{self, Tlv, tag},
};
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
use printcraft_cos::PublicKeyAuth;

const ENVELOPED: &str = "1.2.840.113549.1.7.3";
const DATA: &str = "1.2.840.113549.1.7.1";
const RSA: &str = "1.2.840.113549.1.1.1";
const AES256: &str = "2.16.840.1.101.3.4.1.42";

fn bad(s: &str) -> SignError {
    SignError::Malformed(s.into())
}
fn crypto() -> SignError {
    SignError::Crypto("certificate/private key cannot unlock this document".into())
}

/// Only RSA 2048–4096 recipient certificates, valid now and permitted for key transport.
pub fn validate_recipient(cert: &Certificate, now: i64) -> Result<(), SignError> {
    let PublicKey::Rsa { n, .. } = &cert.public_key else {
        return Err(SignError::Unsupported("请选择 RSA 收件人证书，暂不支持 EC 证书加密".into()));
    };
    let n = n.strip_prefix(&[0]).unwrap_or(n);
    let bits = n.len().saturating_mul(8).saturating_sub(n.first().map_or(8, |b| b.leading_zeros() as usize));
    if !(2048..=4096).contains(&bits) {
        return Err(SignError::Unsupported("RSA 证书需为 2048–4096 位".into()));
    }
    if cert.is_ca || cert.key_usage.is_some_and(|u| u & (1 << 2) == 0) {
        return Err(SignError::Unsupported("此证书不允许密钥加密；请选择收件人的加密证书，而非仅签名证书或 CA 证书".into()));
    }
    if now < cert.not_before.unix() || now > cert.not_after.unix() {
        return Err(SignError::Unsupported("证书尚未生效或已过期".into()));
    }
    Ok(())
}

/// Prepare envelopes with one permission set for every recipient. Uses OS cryptographic RNG.
pub fn recipients(certs: &[Certificate], full_control: bool, now: i64) -> Result<(Vec<Vec<u8>>, PublicKeyAuth), SignError> {
    if certs.is_empty() || certs.len() > 16 {
        return Err(bad("请选择 1–16 个收件人证书"));
    }
    let mut payload = [0u8; 24];
    getrandom::fill(&mut payload[..20]).map_err(|_| SignError::Crypto("secure random generation failed".into()))?;
    // Public-key permissions: bit 1 set, bit 2 owner; reserved 7,8,13..32 cleared.
    let permissions: u32 = if full_control { 0x0F3F } else { 0x0A05 }; // read + high-quality print + accessibility
    payload[20..].copy_from_slice(&permissions.to_be_bytes());
    let mut out = Vec::new();
    for (i, cert) in certs.iter().enumerate() {
        validate_recipient(cert, now)?;
        if certs.iter().take(i).any(|c| c.raw == cert.raw) {
            return Err(bad("duplicate recipient"));
        }
        out.push(envelope(cert, &payload)?);
    }
    Ok((out, PublicKeyAuth::from_payload(&payload).map_err(|e| bad(&e.to_string()))?))
}

#[cfg(not(target_arch = "wasm32"))]
fn envelope(cert: &Certificate, payload: &[u8; 24]) -> Result<Vec<u8>, SignError> {
    use aws_lc_rs::rsa::{Pkcs1PublicEncryptingKey, PublicEncryptingKey};
    let mut key = [0u8; 32];
    let mut iv = [0u8; 16];
    getrandom::fill(&mut key).map_err(|_| crypto())?;
    getrandom::fill(&mut iv).map_err(|_| crypto())?;
    let encrypted = cbc::Encryptor::<aes::Aes256>::new((&key).into(), (&iv).into()).encrypt_padded_vec::<Pkcs7>(payload);
    let rsa = Pkcs1PublicEncryptingKey::new(PublicEncryptingKey::from_der(&cert.public_key.spki()).map_err(|_| crypto())?).map_err(|_| crypto())?;
    let mut wrapped = vec![0u8; rsa.key_size_bytes()];
    let wrapped = rsa.encrypt(&key, &mut wrapped).map_err(|_| crypto())?;
    let rid = der::seq(&[&cert.issuer.raw, &der::uint(&cert.serial)]);
    let recipient = der::seq(&[&der::int(0), &rid, &der::algorithm(RSA, Some(&der::null())), &der::octets(wrapped)]);
    let encrypted_info = der::seq(&[&der::oid(DATA), &der::algorithm(AES256, Some(&der::octets(&iv))), &der::tlv(tag::ctx_prim(0), &encrypted)]);
    let env = der::seq(&[&der::int(0), &der::set_of(&[&recipient]), &encrypted_info]);
    Ok(der::seq(&[&der::oid(ENVELOPED), &der::explicit(0, &env)]))
}

#[cfg(target_arch = "wasm32")]
fn envelope(_: &Certificate, _: &[u8; 24]) -> Result<Vec<u8>, SignError> {
    Err(SignError::Unsupported("certificate encryption requires the desktop app".into()))
}

/// Authenticate one of the recipient CMS envelopes with an imported PKCS#12 private key.
pub fn unlock(envelopes: &[Vec<u8>], id: &DigitalId) -> Result<PublicKeyAuth, SignError> {
    if envelopes.is_empty() || envelopes.len() > 16 || envelopes.iter().any(|e| e.is_empty() || e.len() > 16_384) {
        return Err(bad("invalid recipient envelopes"));
    }
    for e in envelopes {
        if let Some(payload) = decrypt_envelope(e, id)? {
            return PublicKeyAuth::from_payload(&payload).map_err(|e| bad(&e.to_string()));
        }
    }
    Err(crypto())
}

fn child<'a>(a: &[Tlv<'a>], i: usize) -> Result<Tlv<'a>, SignError> {
    a.get(i).copied().ok_or_else(|| bad("truncated CMS envelope"))
}

#[cfg(not(target_arch = "wasm32"))]
fn decrypt_envelope(bytes: &[u8], id: &DigitalId) -> Result<Option<Vec<u8>>, SignError> {
    use aws_lc_rs::rsa::{Pkcs1PrivateDecryptingKey, PrivateDecryptingKey};
    let info = Tlv::parse_all(bytes)?.expect(tag::SEQUENCE, "ContentInfo")?.children()?;
    if child(&info, 0)?.oid()? != ENVELOPED {
        return Err(bad("CMS is not EnvelopedData"));
    }
    let env = child(&info, 1)?.expect(tag::ctx(0), "ContentInfo content")?.inner()?.expect(tag::SEQUENCE, "EnvelopedData")?.children()?;
    if child(&env, 0)?.u64()? != 0 {
        return Err(SignError::Unsupported("only CMS EnvelopedData version 0 is supported".into()));
    }
    let recipients = child(&env, 1)?.expect(tag::SET, "RecipientInfos")?.children()?;
    if recipients.len() > 16 {
        return Err(bad("too many CMS recipients"));
    }
    for r in recipients {
        let r = r.expect(tag::SEQUENCE, "KeyTransRecipientInfo")?.children()?;
        if child(&r, 0)?.u64()? != 0 {
            return Err(bad("unsupported CMS recipient version"));
        }
        let rid = child(&r, 1)?.expect(tag::SEQUENCE, "IssuerAndSerialNumber")?.children()?;
        let serial = child(&rid, 1)?.expect(tag::INTEGER, "serial")?.uint_bytes();
        let own = id.certificate.serial.strip_prefix(&[0]).unwrap_or(&id.certificate.serial);
        if child(&rid, 0)?.raw != id.certificate.issuer.raw || serial != own {
            continue;
        }
        if child(&r, 2)?.children()?.first().ok_or_else(|| bad("missing key algorithm"))?.oid()? != RSA {
            return Err(SignError::Unsupported("only RSA key transport is supported".into()));
        }
        let rsa = Pkcs1PrivateDecryptingKey::new(PrivateDecryptingKey::from_pkcs8(id.key.pkcs8()).map_err(|_| crypto())?).map_err(|_| crypto())?;
        let mut key = vec![0u8; rsa.min_output_size()];
        let key = rsa.decrypt(child(&r, 3)?.expect(tag::OCTET_STRING, "encrypted key")?.value, &mut key).map_err(|_| crypto())?;
        if key.len() != 32 {
            return Err(crypto());
        }
        let encrypted = child(&env, 2)?.expect(tag::SEQUENCE, "EncryptedContentInfo")?.children()?;
        if child(&encrypted, 0)?.oid()? != DATA {
            return Err(bad("unsupported CMS content"));
        }
        let alg = child(&encrypted, 1)?.children()?;
        if child(&alg, 0)?.oid()? != AES256 {
            return Err(SignError::Unsupported("only AES-256-CBC CMS envelopes are supported".into()));
        }
        let iv = child(&alg, 1)?.expect(tag::OCTET_STRING, "AES IV")?.value;
        if iv.len() != 16 {
            return Err(bad("invalid AES IV"));
        }
        let content = child(&encrypted, 2)?.expect(tag::ctx_prim(0), "encrypted content")?.value;
        let clear = cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
            .map_err(|_| crypto())?
            .decrypt_padded_vec::<Pkcs7>(content)
            .map_err(|_| crypto())?;
        if clear.len() != 24 {
            return Err(crypto());
        }
        return Ok(Some(clear));
    }
    Ok(None)
}

#[cfg(target_arch = "wasm32")]
fn decrypt_envelope(_: &[u8], _: &DigitalId) -> Result<Option<Vec<u8>>, SignError> {
    Err(SignError::Unsupported("certificate decryption requires the desktop app".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cms_roundtrip_and_wrong_identity() {
        let mut id = crate::pkcs12::open(include_bytes!("../tests/data/rsa-aes.p12"), "test").unwrap();
        let now = 1_791_417_600;
        id.certificate =
            Certificate::self_signed_encryption(&id.certificate.subject, &id.key, crate::Time::from_unix(now - 3600), 5, &[0x42]).unwrap();
        let (envelopes, _) = recipients(std::slice::from_ref(&id.certificate), true, now).unwrap();
        unlock(&envelopes, &id).unwrap();
        let wrong = crate::pkcs12::open(include_bytes!("../tests/data/ec-p256.p12"), "test").unwrap();
        assert!(unlock(&envelopes, &wrong).is_err());
        assert!(validate_recipient(&wrong.certificate, now).is_err());
        let sig = crate::pkcs12::open(include_bytes!("../tests/data/chain.p12"), "test").unwrap();
        assert!(validate_recipient(&sig.certificate, now).is_err());
        assert!(unlock(&[vec![0; 20_000]], &id).is_err());
        for n in 0..envelopes[0].len() {
            assert!(unlock(&[envelopes[0][..n].to_vec()], &id).is_err());
        }
    }

    #[test]
    #[ignore = "external OpenSSL CMS interoperability oracle"]
    fn cms_openssl_interoperability() {
        use std::process::Command;
        let mut id = crate::pkcs12::open(include_bytes!("../tests/data/rsa-aes.p12"), "test").unwrap();
        let now = 1_791_417_600;
        id.certificate =
            Certificate::self_signed_encryption(&id.certificate.subject, &id.key, crate::Time::from_unix(now - 3600), 5, &[0x42]).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "myaipdf-cms-oracle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let (envelopes, auth) = recipients(std::slice::from_ref(&id.certificate), true, now).unwrap();
        std::fs::write(dir.join("recipient.der"), &id.certificate.raw).unwrap();
        std::fs::write(dir.join("key.der"), id.key.pkcs8()).unwrap();
        std::fs::write(dir.join("ours.cms"), &envelopes[0]).unwrap();
        let run = |args: &[&str]| {
            let out = Command::new("openssl").current_dir(&dir).args(args).output().unwrap();
            assert!(out.status.success(), "openssl: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["x509", "-inform", "DER", "-in", "recipient.der", "-out", "recipient.pem"]);
        run(&[
            "cms",
            "-decrypt",
            "-binary",
            "-inform",
            "DER",
            "-in",
            "ours.cms",
            "-recip",
            "recipient.pem",
            "-inkey",
            "key.der",
            "-keyform",
            "DER",
            "-out",
            "payload.bin",
        ]);
        let clear = std::fs::read(dir.join("payload.bin")).unwrap();
        assert_eq!(PublicKeyAuth::from_payload(&clear).unwrap(), auth);
        run(&["cms", "-encrypt", "-binary", "-aes256", "-in", "payload.bin", "-outform", "DER", "-out", "oracle.cms", "recipient.pem"]);
        assert_eq!(unlock(&[std::fs::read(dir.join("oracle.cms")).unwrap()], &id).unwrap(), auth);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
