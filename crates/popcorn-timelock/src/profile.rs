//! POPCORN-TLOCK-AGE-V1 (SPEC.md §3.6): the acceptance policy for encrypted blobs.
//!
//! age is an extensible format, and extensibility is precisely what a consensus rule cannot
//! afford: if one implementation accepts a header another rejects, the two disagree about
//! which transactions exist. So POPCORN accepts one profile and validates it *before*
//! decryption — the check runs on bytes an attacker chose, so it parses defensively and
//! never panics.

/// Maximum header size, from the first byte through the MAC line (§3.6).
pub const MAX_HEADER_SIZE: usize = 1024;

/// The age v1 intro line; the only one accepted.
const AGE_V1_INTRO: &[u8] = b"age-encryption.org/v1";
/// The armored envelope marker. Armor is forbidden: it is a second encoding of the same
/// ciphertext, and two encodings mean two possible hashes for one blob.
const ARMOR_MARKER: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";
/// The recipient stanza type this profile accepts.
const TLOCK_STANZA: &str = "tlock";
/// Suffix of the "grease" stanza that age appends to every header it writes.
///
/// This is not an indulgence: `age` adds a randomized stanza tagged `<random>-grease` to
/// every non-scrypt header, deliberately, to keep parsers from ossifying. Refusing it would
/// reject every blob produced by the Rust client this specification names as interoperable.
/// It carries no key material — no implementation unwraps a file key from an unknown tag —
/// so it cannot become a second decryption path; it is bounded by the header size cap like
/// everything else. See §3.6 and §14.12.
const GREASE_SUFFIX: &str = "-grease";

/// Why a blob is outside the profile. Every variant resolves to `unusable` (§5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    Empty,
    Armored,
    BadIntroLine,
    CarriageReturn,
    HeaderTooLarge(usize),
    NoStanza,
    /// More than one `tlock` stanza: which one is the recipient would be ambiguous.
    MultipleTlockStanzas(usize),
    /// A stanza that is neither `tlock` nor grease — a possible second decryption path.
    ForeignStanza(String),
    /// More than one grease stanza: age writes at most one.
    MultipleGreaseStanzas(usize),
    WrongStanzaType(String),
    MalformedStanzaArgs,
    NonCanonicalRound,
    RoundMismatch {
        expected: u64,
        found: u64,
    },
    ChainHashMismatch,
    NonCanonicalBase64,
    MissingMac,
    NoPayload,
}

/// What a conforming header says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobHeader {
    pub round: u64,
    pub chain_hash: [u8; 32],
    /// Byte length of the header, MAC line included.
    pub header_len: usize,
}

/// Validate a blob against the profile and return what its header claims.
///
/// `expected_round` and `expected_chain` are checked here rather than after decryption: a
/// blob aimed at another round or another drand chain is not a late transaction, it is not a
/// transaction at all.
pub fn validate(
    blob: &[u8],
    expected_round: u64,
    expected_chain: &[u8; 32],
) -> Result<BlobHeader, ProfileError> {
    if blob.is_empty() {
        return Err(ProfileError::Empty);
    }
    if blob.starts_with(ARMOR_MARKER) {
        return Err(ProfileError::Armored);
    }

    // Walk the header line by line. The header ends at the MAC line that starts with "---".
    let mut offset = 0usize;
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut mac_seen = false;
    while offset < blob.len() {
        let end = match blob[offset..].iter().position(|b| *b == b'\n') {
            Some(index) => offset + index,
            None => return Err(ProfileError::MissingMac),
        };
        let line = &blob[offset..end];
        if line.contains(&b'\r') {
            // LF only: a CRLF header is a second encoding of the same bytes.
            return Err(ProfileError::CarriageReturn);
        }
        lines.push(line);
        offset = end + 1;
        if line.starts_with(b"---") {
            mac_seen = true;
            break;
        }
        if offset > MAX_HEADER_SIZE {
            return Err(ProfileError::HeaderTooLarge(offset));
        }
    }

    if !mac_seen {
        return Err(ProfileError::MissingMac);
    }
    let header_len = offset;
    if header_len > MAX_HEADER_SIZE {
        return Err(ProfileError::HeaderTooLarge(header_len));
    }
    if header_len >= blob.len() {
        // A header with no payload cannot carry a transaction.
        return Err(ProfileError::NoPayload);
    }

    if lines.first().map(|l| *l != AGE_V1_INTRO).unwrap_or(true) {
        return Err(ProfileError::BadIntroLine);
    }

    // Exactly one `tlock` stanza, at most one grease stanza, nothing else (§3.6).
    let stanza_lines: Vec<&[u8]> = lines
        .iter()
        .filter(|line| line.starts_with(b"-> "))
        .copied()
        .collect();
    if stanza_lines.is_empty() {
        return Err(ProfileError::NoStanza);
    }

    let mut tlock_stanza: Option<&str> = None;
    let mut tlock_count = 0usize;
    let mut grease_count = 0usize;
    for line in &stanza_lines {
        let stanza =
            std::str::from_utf8(&line[3..]).map_err(|_| ProfileError::MalformedStanzaArgs)?;
        let stanza_type = stanza
            .split(' ')
            .next()
            .ok_or(ProfileError::MalformedStanzaArgs)?;
        if stanza_type == TLOCK_STANZA {
            tlock_count += 1;
            tlock_stanza = Some(stanza);
        } else if stanza_type.ends_with(GREASE_SUFFIX) {
            grease_count += 1;
        } else {
            // Anything else could be a recipient someone else can unwrap.
            return Err(ProfileError::ForeignStanza(stanza_type.to_string()));
        }
    }
    if tlock_count == 0 {
        return Err(ProfileError::WrongStanzaType(String::new()));
    }
    if tlock_count > 1 {
        return Err(ProfileError::MultipleTlockStanzas(tlock_count));
    }
    if grease_count > 1 {
        return Err(ProfileError::MultipleGreaseStanzas(grease_count));
    }

    let mut args = tlock_stanza.expect("exactly one tlock stanza").split(' ');
    let _ = args.next();
    let round_arg = args.next().ok_or(ProfileError::MalformedStanzaArgs)?;
    let chain_arg = args.next().ok_or(ProfileError::MalformedStanzaArgs)?;
    if args.next().is_some() {
        return Err(ProfileError::MalformedStanzaArgs);
    }

    // A canonical decimal: "007" and "7" would be two encodings of one round, hence two
    // blob hashes for one logical submission.
    if round_arg.is_empty() || (round_arg.len() > 1 && round_arg.starts_with('0')) {
        return Err(ProfileError::NonCanonicalRound);
    }
    let round: u64 = round_arg
        .parse()
        .map_err(|_| ProfileError::NonCanonicalRound)?;
    if round != expected_round {
        return Err(ProfileError::RoundMismatch {
            expected: expected_round,
            found: round,
        });
    }

    let chain_hash = parse_lowercase_hex32(chain_arg).ok_or(ProfileError::ChainHashMismatch)?;
    if chain_hash != *expected_chain {
        return Err(ProfileError::ChainHashMismatch);
    }

    // Stanza body and MAC are unpadded standard base64 (§3.6).
    for line in lines.iter().skip(1) {
        let body = if line.starts_with(b"---") {
            &line[3..]
        } else if line.starts_with(b"-> ") {
            continue;
        } else {
            line
        };
        let body = body.strip_prefix(b" ").unwrap_or(body);
        if !is_canonical_base64(body) {
            return Err(ProfileError::NonCanonicalBase64);
        }
    }

    Ok(BlobHeader {
        round,
        chain_hash,
        header_len,
    })
}

/// Lowercase hex, exactly 32 bytes. Uppercase is a second encoding and is refused.
fn parse_lowercase_hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let bytes = text.as_bytes();
    for (index, slot) in out.iter_mut().enumerate() {
        let hi = hex_digit(bytes[index * 2])?;
        let lo = hex_digit(bytes[index * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None, // uppercase hex is not canonical here
    }
}

/// Unpadded standard base64, as age writes it.
fn is_canonical_base64(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
}
