// popcorn-interop is the Go half of the cross-language gate of SPEC.md §2.4.
//
// It uses drand's own tlock implementation (github.com/drand/tlock) over filippo.io/age,
// which is a genuinely independent code path from the Rust `tlock_age`/`age` stack: a
// round-trip that works in both directions is evidence about the *format*, not about one
// library agreeing with itself.
//
// The network is offline by construction. Encryption needs only the chain's public key, and
// decryption only the round's signature — both supplied on the command line — so this tool
// never talks to drand and the gate never depends on drand being up.
//
//	popcorn-interop encrypt <round> <plaintext-hex>
//	popcorn-interop decrypt <blob-hex> <signature-hex>
//	popcorn-interop profile <blob-hex> <round> <chain-hash-hex>
package main

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/drand/drand/v2/crypto"
	"github.com/drand/kyber"
	"github.com/drand/tlock"
)

// Pinned at genesis (SPEC.md §2.4). quicknet: unchained signatures on G1.
const (
	chainHashHex  = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971"
	publicKeyHex  = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a"
	schemeID      = crypto.SigsOnG1ID
	genesisTime   = 1692803367
	periodSeconds = 3
)

// offlineNetwork satisfies tlock.Network without any I/O.
//
// Signature() returns whatever the caller passed in, which is the whole point: a verifier
// replaying a block already holds the round's signature, so decryption must not require a
// live beacon.
type offlineNetwork struct {
	scheme    *crypto.Scheme
	publicKey kyber.Point
	signature []byte
}

func (n *offlineNetwork) ChainHash() string     { return chainHashHex }
func (n *offlineNetwork) PublicKey() kyber.Point { return n.publicKey }
func (n *offlineNetwork) Scheme() crypto.Scheme  { return *n.scheme }

func (n *offlineNetwork) Current(t time.Time) uint64 {
	if t.Unix() <= genesisTime {
		return 1
	}
	return uint64((t.Unix()-genesisTime)/periodSeconds) + 1
}

func (n *offlineNetwork) Signature(round uint64) ([]byte, error) {
	if n.signature == nil {
		return nil, fmt.Errorf("no signature was supplied for round %d", round)
	}
	return n.signature, nil
}

func (n *offlineNetwork) SwitchChainHash(hash string) error {
	if hash != chainHashHex {
		return fmt.Errorf("refusing to switch away from the pinned chain hash")
	}
	return nil
}

func newNetwork(signature []byte) (*offlineNetwork, error) {
	scheme, err := crypto.SchemeFromName(schemeID)
	if err != nil {
		return nil, fmt.Errorf("scheme %s: %w", schemeID, err)
	}
	keyBytes, err := hex.DecodeString(publicKeyHex)
	if err != nil {
		return nil, err
	}
	point := scheme.KeyGroup.Point()
	if err := point.UnmarshalBinary(keyBytes); err != nil {
		return nil, fmt.Errorf("public key: %w", err)
	}
	return &offlineNetwork{scheme: scheme, publicKey: point, signature: signature}, nil
}

func main() {
	if len(os.Args) < 2 {
		fail("usage: popcorn-interop <encrypt|decrypt|profile> ...")
	}

	switch os.Args[1] {
	case "encrypt":
		if len(os.Args) != 4 {
			fail("usage: popcorn-interop encrypt <round> <plaintext-hex>")
		}
		round, err := strconv.ParseUint(os.Args[2], 10, 64)
		check(err)
		plaintext, err := hex.DecodeString(os.Args[3])
		check(err)

		network, err := newNetwork(nil)
		check(err)
		var out bytes.Buffer
		check(tlock.New(network).Encrypt(&out, bytes.NewReader(plaintext), round))
		fmt.Println(hex.EncodeToString(out.Bytes()))

	case "decrypt":
		if len(os.Args) != 4 {
			fail("usage: popcorn-interop decrypt <blob-hex> <signature-hex>")
		}
		blob, err := hex.DecodeString(os.Args[2])
		check(err)
		signature, err := hex.DecodeString(os.Args[3])
		check(err)

		network, err := newNetwork(signature)
		check(err)
		var out bytes.Buffer
		// Strict() keeps the chain hash in the ciphertext from redirecting us to another
		// drand chain — the Go equivalent of the pinning §3.6 requires.
		check(tlock.New(network).Strict().Decrypt(&out, bytes.NewReader(blob)))
		fmt.Println(hex.EncodeToString(out.Bytes()))

	case "profile":
		if len(os.Args) != 5 {
			fail("usage: popcorn-interop profile <blob-hex> <round> <chain-hash-hex>")
		}
		blob, err := hex.DecodeString(os.Args[2])
		check(err)
		round, err := strconv.ParseUint(os.Args[3], 10, 64)
		check(err)
		if reason := validateProfile(blob, round, os.Args[4]); reason != "" {
			fmt.Println(reason)
			os.Exit(2)
		}
		fmt.Println("OK")

	default:
		fail("unknown command " + os.Args[1])
	}
}

// validateProfile is an independent implementation of POPCORN-TLOCK-AGE-V1 (SPEC.md §3.6).
//
// It exists to prove the acceptance policy is implementable from the specification rather
// than only from the Rust source: node and verifiers must accept and reject the same bytes,
// and two implementations that disagree about which blobs are `unusable` disagree about
// which transactions exist. It returns the error name, or "" when the blob conforms.
func validateProfile(blob []byte, expectedRound uint64, expectedChain string) string {
	if len(blob) == 0 {
		return "Empty"
	}
	if bytes.HasPrefix(blob, []byte("-----BEGIN AGE ENCRYPTED FILE-----")) {
		return "Armored"
	}

	var lines [][]byte
	offset := 0
	macSeen := false
	for offset < len(blob) {
		index := bytes.IndexByte(blob[offset:], '\n')
		if index < 0 {
			return "MissingMac"
		}
		line := blob[offset : offset+index]
		if bytes.ContainsRune(line, '\r') {
			return "CarriageReturn"
		}
		lines = append(lines, line)
		offset += index + 1
		if bytes.HasPrefix(line, []byte("---")) {
			macSeen = true
			break
		}
		if offset > 1024 {
			return "HeaderTooLarge"
		}
	}
	if !macSeen {
		return "MissingMac"
	}
	if offset > 1024 {
		return "HeaderTooLarge"
	}
	if offset >= len(blob) {
		return "NoPayload"
	}
	if !bytes.Equal(lines[0], []byte("age-encryption.org/v1")) {
		return "BadIntroLine"
	}

	var tlockStanza string
	tlockCount, greaseCount := 0, 0
	for _, line := range lines {
		if !bytes.HasPrefix(line, []byte("-> ")) {
			continue
		}
		stanza := string(line[3:])
		stanzaType := strings.SplitN(stanza, " ", 2)[0]
		switch {
		case stanzaType == "tlock":
			tlockCount++
			tlockStanza = stanza
		case strings.HasSuffix(stanzaType, "-grease"):
			greaseCount++
		default:
			// A foreign stanza would be a decryption path for someone other than the round.
			return "ForeignStanza"
		}
	}
	switch {
	case tlockCount == 0 && greaseCount == 0:
		return "NoStanza"
	case tlockCount == 0:
		return "WrongStanzaType"
	case tlockCount > 1:
		return "MultipleTlockStanzas"
	case greaseCount > 1:
		return "MultipleGreaseStanzas"
	}

	args := strings.Split(tlockStanza, " ")
	if len(args) != 3 {
		return "MalformedStanzaArgs"
	}
	roundArg, chainArg := args[1], args[2]
	if roundArg == "" || (len(roundArg) > 1 && roundArg[0] == '0') {
		return "NonCanonicalRound"
	}
	round, err := strconv.ParseUint(roundArg, 10, 64)
	if err != nil {
		return "NonCanonicalRound"
	}
	if round != expectedRound {
		return "RoundMismatch"
	}
	if len(chainArg) != 64 || strings.ToLower(chainArg) != chainArg || chainArg != expectedChain {
		return "ChainHashMismatch"
	}
	if _, err := hex.DecodeString(chainArg); err != nil {
		return "ChainHashMismatch"
	}

	for _, line := range lines[1:] {
		body := line
		if bytes.HasPrefix(body, []byte("-> ")) {
			continue
		}
		body = bytes.TrimPrefix(body, []byte("---"))
		body = bytes.TrimPrefix(body, []byte(" "))
		for _, c := range body {
			isBase64 := (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
				(c >= '0' && c <= '9') || c == '+' || c == '/'
			if !isBase64 {
				return "NonCanonicalBase64"
			}
		}
	}
	return ""
}

func check(err error) {
	if err != nil {
		fail(err.Error())
	}
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, "error: "+message)
	os.Exit(1)
}
