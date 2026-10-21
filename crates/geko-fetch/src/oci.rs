//! Pull of an OCI artifact, such as a country pack, from a registry such as ghcr.io (PROJECT.md
//! §2.4), with the distribution API: an anonymous token when the registry asks for one, the
//! manifest, then each layer, checked against its digest. Each layer is a file, named by its
//! `org.opencontainers.image.title` annotation, as `oras push` leaves them.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use geko_module_sdk::{GekoError, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The digest and the body of a manifest, and the token it took.
type Fetched = (String, Vec<u8>, Option<String>);

const MANIFEST_TYPES: &str = "application/vnd.oci.image.manifest.v1+json";
const TITLE: &str = "org.opencontainers.image.title";
/// No file of an artifact may be larger: a country pack is a few megabytes.
const MAX_BLOB: u64 = 256 << 20;

/// `<host>/<repository>:<tag>`, e.g. `ghcr.io/candymaps/geko/countries/es:format-1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub host: String,
    pub repository: String,
    pub tag: String,
}

impl Reference {
    pub fn parse(reference: &str) -> Result<Self> {
        let invalid = || GekoError::Config(format!("`{reference}` is not an OCI reference (<host>/<repository>:<tag>)"));
        let (host, rest) = reference.split_once('/').ok_or_else(invalid)?;
        let (repository, tag) = rest.rsplit_once(':').ok_or_else(invalid)?;
        if host.is_empty() || repository.is_empty() || tag.is_empty() || tag.contains('/') {
            return Err(invalid());
        }
        Ok(Self { host: host.to_owned(), repository: repository.to_owned(), tag: tag.to_owned() })
    }

    /// A registry on this machine is reached over plain HTTP, any other over HTTPS.
    fn base(&self) -> String {
        let local = self.host.starts_with("localhost") || self.host.starts_with("127.0.0.1");
        format!("{}://{}/v2/{}", if local { "http" } else { "https" }, self.host, self.repository)
    }
}

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.host, self.repository, self.tag)
    }
}

/// The files of an artifact, by relative path, and the digest of its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub digest: String,
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Deserialize)]
struct Manifest {
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Layer {
    digest: String,
    #[serde(default)]
    annotations: HashMap<String, String>,
}

pub struct OciClient {
    agent: ureq::Agent,
}

impl Default for OciClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OciClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout_read(Duration::from_secs(120))
            .user_agent(concat!("geko/", env!("CARGO_PKG_VERSION")))
            .build();
        Self { agent }
    }

    /// The digest of the manifest of `reference`, or None if the registry has no such artifact
    /// for an anonymous client: missing, or not public.
    pub fn digest(&self, reference: &Reference) -> Result<Option<String>> {
        Ok(self.manifest(reference)?.map(|(digest, _, _)| digest))
    }

    /// The artifact, or None as for `digest`. Every file is checked against its digest.
    pub fn pull(&self, reference: &Reference) -> Result<Option<Artifact>> {
        let Some((digest, body, token)) = self.manifest(reference)? else { return Ok(None) };
        let manifest: Manifest = serde_json::from_slice(&body)
            .map_err(|e| GekoError::Data(format!("{reference}: invalid manifest: {e}")))?;
        let mut files = Vec::with_capacity(manifest.layers.len());
        for layer in &manifest.layers {
            let path = layer
                .annotations
                .get(TITLE)
                .ok_or_else(|| GekoError::Data(format!("{reference}: layer {} has no {TITLE}", layer.digest)))?;
            if path.is_empty() || path.starts_with('/') || path.split(['/', '\\']).any(|part| part == ".." || part.is_empty()) {
                return Err(GekoError::Data(format!("{reference}: file `{path}` leaves the artifact")));
            }
            let url = format!("{}/blobs/{}", reference.base(), layer.digest);
            let bytes = self.get(&url, token.as_deref(), None)?.ok_or_else(|| GekoError::Data(format!("GET {url}: not found")))?;
            let actual = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
            if actual != layer.digest {
                return Err(GekoError::Data(format!("{reference}: {path} is {actual}, not {}", layer.digest)));
            }
            files.push((path.clone(), bytes));
        }
        Ok(Some(Artifact { digest, files }))
    }

    /// The digest, the body and the token of the manifest.
    fn manifest(&self, reference: &Reference) -> Result<Option<Fetched>> {
        let url = format!("{}/manifests/{}", reference.base(), reference.tag);
        let first = self.agent.get(&url).set("Accept", MANIFEST_TYPES).call();
        let (body, token) = match first {
            Ok(response) => (read(response, &url)?, None),
            Err(ureq::Error::Status(401, response)) => {
                let challenge = response.header("WWW-Authenticate").unwrap_or_default().to_owned();
                let Some(token) = self.token(&challenge, &url)? else { return Ok(None) };
                match self.get(&url, Some(&token), Some(MANIFEST_TYPES))? {
                    Some(body) => (body, Some(token)),
                    None => return Ok(None),
                }
            }
            Err(ureq::Error::Status(403 | 404, _)) => return Ok(None),
            Err(e) => return Err(GekoError::Data(format!("GET {url}: {e}"))),
        };
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&body)));
        Ok(Some((digest, body, token)))
    }

    /// An anonymous token from the realm of a `Bearer` challenge; None if the registry denies it.
    fn token(&self, challenge: &str, url: &str) -> Result<Option<String>> {
        let params = challenge_params(challenge);
        let realm = params.get("realm").ok_or_else(|| GekoError::Data(format!("GET {url}: 401 without a Bearer realm")))?;
        let mut request = self.agent.get(realm);
        for key in ["service", "scope"] {
            if let Some(value) = params.get(key) {
                request = request.query(key, value);
            }
        }
        #[derive(Deserialize)]
        struct Token {
            token: Option<String>,
            access_token: Option<String>,
        }
        let response = match request.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(401 | 403 | 404, _)) => return Ok(None),
            Err(e) => return Err(GekoError::Data(format!("GET {realm}: {e}"))),
        };
        let token: Token = serde_json::from_slice(&read(response, realm)?)
            .map_err(|e| GekoError::Data(format!("GET {realm}: invalid token: {e}")))?;
        Ok(token.token.or(token.access_token))
    }

    /// The body of `url`, or None if the registry says it is not there or not for us.
    fn get(&self, url: &str, token: Option<&str>, accept: Option<&str>) -> Result<Option<Vec<u8>>> {
        let mut request = self.agent.get(url);
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        if let Some(accept) = accept {
            request = request.set("Accept", accept);
        }
        match request.call() {
            Ok(response) => Ok(Some(read(response, url)?)),
            Err(ureq::Error::Status(401 | 403 | 404, _)) => Ok(None),
            Err(e) => Err(GekoError::Data(format!("GET {url}: {e}"))),
        }
    }
}

fn read(response: ureq::Response, url: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BLOB + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| GekoError::Data(format!("GET {url}: {e}")))?;
    if bytes.len() as u64 > MAX_BLOB {
        return Err(GekoError::Data(format!("GET {url}: larger than {MAX_BLOB} bytes")));
    }
    Ok(bytes)
}

/// `Bearer realm="…",service="…",scope="…"` → its parameters.
fn challenge_params(challenge: &str) -> HashMap<String, String> {
    let rest = challenge.trim().strip_prefix("Bearer").unwrap_or(challenge).trim();
    let mut params = HashMap::new();
    let mut chars = rest.chars().peekable();
    while chars.peek().is_some() {
        let key: String = chars.by_ref().take_while(|c| *c != '=').collect::<String>().trim().trim_start_matches(',').trim().to_owned();
        let value: String = if chars.peek() == Some(&'"') {
            chars.next();
            let v = chars.by_ref().take_while(|c| *c != '"').collect();
            while chars.peek().is_some_and(|c| *c == ',' || c.is_whitespace()) {
                chars.next();
            }
            v
        } else {
            chars.by_ref().take_while(|c| *c != ',').collect::<String>().trim().to_owned()
        };
        if !key.is_empty() {
            params.insert(key, value);
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use std::thread;

    use tiny_http::{Header, Response, Server};

    use super::*;

    fn digest(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    /// A registry that asks for a token, as ghcr.io does, serving one artifact at `es:format-1`.
    fn registry(files: &[(&str, &[u8])], tamper: bool) -> String {
        let server = Server::http("127.0.0.1:0").unwrap();
        let host = format!("127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
        let layers: Vec<String> = files
            .iter()
            .map(|(name, bytes)| format!(r#"{{"mediaType":"application/octet-stream","digest":"{}","size":{},"annotations":{{"{TITLE}":"{name}"}}}}"#, digest(bytes), bytes.len()))
            .collect();
        let manifest = format!(r#"{{"schemaVersion":2,"layers":[{}]}}"#, layers.join(","));
        let blobs: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(_, bytes)| (digest(bytes), if tamper { b"other".to_vec() } else { bytes.to_vec() }))
            .collect();
        let realm = format!("http://{host}/token");
        thread::spawn(move || {
            for request in server.incoming_requests() {
                let url = request.url().to_owned();
                let authorized = request.headers().iter().any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer t0k");
                let response = if url.starts_with("/token") {
                    assert!(url.contains("scope=repository%3Acandymaps%2Fgeko%2Fcountries%2Fes%3Apull"), "{url}");
                    Response::from_string(r#"{"token":"t0k"}"#)
                } else if !authorized {
                    let challenge = format!(r#"Bearer realm="{realm}",service="test",scope="repository:candymaps/geko/countries/es:pull""#);
                    Response::from_string("").with_status_code(401).with_header(Header::from_bytes("WWW-Authenticate", challenge).unwrap())
                } else if url == "/v2/candymaps/geko/countries/es/manifests/format-1" {
                    Response::from_string(manifest.clone())
                } else if let Some(d) = url.strip_prefix("/v2/candymaps/geko/countries/es/blobs/") {
                    Response::from_data(blobs[d].clone())
                } else {
                    Response::from_string("").with_status_code(404)
                };
                let _ = request.respond(response);
            }
        });
        host
    }

    #[test]
    fn parses_references() {
        let r = Reference::parse("ghcr.io/candymaps/geko/countries/es:format-1").unwrap();
        assert_eq!((r.host.as_str(), r.repository.as_str(), r.tag.as_str()), ("ghcr.io", "candymaps/geko/countries/es", "format-1"));
        assert_eq!(r.to_string(), "ghcr.io/candymaps/geko/countries/es:format-1");
        assert!(Reference::parse("ghcr.io/candymaps/geko").is_err());
        assert!(Reference::parse("es:1").is_err());
    }

    #[test]
    fn reads_bearer_challenges() {
        let p = challenge_params(r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:a/b:pull""#);
        assert_eq!(p["realm"], "https://ghcr.io/token");
        assert_eq!(p["service"], "ghcr.io");
        assert_eq!(p["scope"], "repository:a/b:pull");
    }

    #[test]
    fn pulls_an_artifact_with_an_anonymous_token() {
        let files: [(&str, &[u8]); 2] = [("pack.toml", b"country = \"ES\""), ("data/words.toml", b"connectors = []")];
        let host = registry(&files, false);
        let client = OciClient::new();
        let reference = Reference::parse(&format!("{host}/candymaps/geko/countries/es:format-1")).unwrap();
        let artifact = client.pull(&reference).unwrap().unwrap();
        assert_eq!(artifact.files, files.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect::<Vec<_>>());
        assert_eq!(client.digest(&reference).unwrap().as_deref(), Some(artifact.digest.as_str()));

        let missing = Reference::parse(&format!("{host}/candymaps/geko/countries/es:format-9")).unwrap();
        assert_eq!(client.pull(&missing).unwrap(), None);
    }

    #[test]
    fn rejects_a_file_that_does_not_match_its_digest() {
        let host = registry(&[("pack.toml", b"country = \"ES\"")], true);
        let reference = Reference::parse(&format!("{host}/candymaps/geko/countries/es:format-1")).unwrap();
        let err = OciClient::new().pull(&reference).unwrap_err();
        assert!(err.to_string().contains("not sha256:"), "{err}");
    }
}
