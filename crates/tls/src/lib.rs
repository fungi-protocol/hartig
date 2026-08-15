//! TLS configuration and certificate loading for Hartig.

use std::{
    fmt,
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
};

use rustls::{ServerConfig, pki_types::PrivateKeyDer};
use tokio_rustls::TlsAcceptor;

#[derive(Debug)]
pub enum Error {
    Open { path: PathBuf, source: io::Error },
    ReadCertificate { path: PathBuf, source: io::Error },
    ReadPrivateKey { path: PathBuf, source: io::Error },
    NoCertificates(PathBuf),
    NoPrivateKey(PathBuf),
    InvalidKeyPair(rustls::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(formatter, "cannot open {}: {source}", path.display())
            }
            Self::ReadCertificate { path, source } => {
                write!(
                    formatter,
                    "cannot read certificates from {}: {source}",
                    path.display()
                )
            }
            Self::ReadPrivateKey { path, source } => {
                write!(
                    formatter,
                    "cannot read private key from {}: {source}",
                    path.display()
                )
            }
            Self::NoCertificates(path) => {
                write!(formatter, "no certificates found in {}", path.display())
            }
            Self::NoPrivateKey(path) => {
                write!(formatter, "no private key found in {}", path.display())
            }
            Self::InvalidKeyPair(source) => {
                write!(formatter, "invalid TLS certificate/key pair: {source}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. }
            | Self::ReadCertificate { source, .. }
            | Self::ReadPrivateKey { source, .. } => Some(source),
            Self::InvalidKeyPair(source) => Some(source),
            Self::NoCertificates(_) | Self::NoPrivateKey(_) => None,
        }
    }
}

/// Loads a PEM certificate chain and private key into a rustls server config.
///
/// The first private key in the key file is used. Supported key encodings are
/// PKCS#1, PKCS#8, and SEC1.
///
/// # Errors
///
/// Returns an error when either file cannot be opened or parsed, when the PEM
/// files contain no usable certificate or private key, or when rustls rejects
/// the certificate and key combination.
pub fn server_config(
    certificate_path: impl AsRef<Path>,
    private_key_path: impl AsRef<Path>,
) -> Result<Arc<ServerConfig>, Error> {
    let certificate_path = certificate_path.as_ref();
    let private_key_path = private_key_path.as_ref();

    let certificate_file = File::open(certificate_path).map_err(|source| Error::Open {
        path: certificate_path.to_owned(),
        source,
    })?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(certificate_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| Error::ReadCertificate {
            path: certificate_path.to_owned(),
            source,
        })?;
    if certificates.is_empty() {
        return Err(Error::NoCertificates(certificate_path.to_owned()));
    }

    let private_key_file = File::open(private_key_path).map_err(|source| Error::Open {
        path: private_key_path.to_owned(),
        source,
    })?;
    let private_key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut BufReader::new(private_key_file))
            .map_err(|source| Error::ReadPrivateKey {
                path: private_key_path.to_owned(),
                source,
            })?
            .ok_or_else(|| Error::NoPrivateKey(private_key_path.to_owned()))?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(Error::InvalidKeyPair)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[must_use]
pub fn acceptor(config: Arc<ServerConfig>) -> TlsAcceptor {
    TlsAcceptor::from(config)
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use rcgen::generate_simple_self_signed;

    use super::{Error, acceptor, server_config};

    #[test]
    fn loads_a_pem_certificate_and_private_key() {
        let generated = generate_simple_self_signed(["localhost".to_owned()]).unwrap();
        let directory = tempfile_dir();
        let certificate_path = directory.join("certificate.pem");
        let key_path = directory.join("key.pem");
        fs::write(&certificate_path, generated.cert.pem()).unwrap();
        fs::write(&key_path, generated.key_pair.serialize_pem()).unwrap();

        let config = server_config(certificate_path, key_path).unwrap();
        assert_eq!(
            config.alpn_protocols,
            [b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        let _acceptor = acceptor(Arc::clone(&config));
    }

    #[test]
    fn rejects_a_file_without_certificates() {
        let directory = tempfile_dir();
        let certificate_path = directory.join("certificate.pem");
        let key_path = directory.join("key.pem");
        fs::write(&certificate_path, "not a certificate").unwrap();
        fs::write(&key_path, "not a key").unwrap();

        assert!(matches!(
            server_config(&certificate_path, &key_path),
            Err(Error::NoCertificates(path)) if path == certificate_path
        ));
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("hartig-tls-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
