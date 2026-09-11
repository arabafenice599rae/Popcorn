"""Ed25519 verification under the semantics SPEC.md §3.1 pins, in pure Python.

This is the piece of the reference executor that most deserves to be written twice. RFC 8032
is under-specified around borderline signatures, implementations diverge, and the divergence
is consensus-visible: a replaying verifier must accept and reject *exactly* the same
signatures as the node, or the two disagree about which transactions exist.

The frozen semantics is `ed25519-dalek`'s `verify_strict`:

* the scalar ``s`` must be canonical (``s < L``);
* neither the public key ``A`` nor the commitment ``R`` may be a small-order point;
* the verification equation is *cofactorless*: ``[s]B == R + [k]A``, without multiplying
  through by the cofactor 8.

No dependencies: only ``hashlib`` from the standard library.
"""

import hashlib

# Curve25519 field and group order.
P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)

# Base point.
_BY = 4 * pow(5, P - 2, P) % P
_BX = None  # filled in below


def _recover_x(y: int, sign: int):
    """The x coordinate matching y, or None if the point is not on the curve."""
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    if x2 == 0:
        # x == 0 is only valid with a zero sign bit; the other encoding is non-canonical.
        return None if sign else 0

    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P != 0:
        return None
    if x % 2 != sign:
        x = P - x
    return x


# Extended homogeneous coordinates (X, Y, Z, T), as in the RFC 8032 reference.
def _point_add(p, q):
    x1, y1, z1, t1 = p
    x2, y2, z2, t2 = q
    a = (y1 - x1) * (y2 - x2) % P
    b = (y1 + x1) * (y2 + x2) % P
    c = 2 * t1 * t2 * D % P
    dd = 2 * z1 * z2 % P
    e, f, g, h = b - a, dd - c, dd + c, b + a
    return (e * f % P, g * h % P, f * g % P, e * h % P)


def _point_double(p):
    return _point_add(p, p)


def _scalar_mult(p, scalar: int):
    result = (0, 1, 1, 0)  # neutral element
    while scalar > 0:
        if scalar & 1:
            result = _point_add(result, p)
        p = _point_double(p)
        scalar >>= 1
    return result


def _point_equal(p, q):
    x1, y1, z1, _ = p
    x2, y2, z2, _ = q
    return (x1 * z2 - x2 * z1) % P == 0 and (y1 * z2 - y2 * z1) % P == 0


_BX = _recover_x(_BY, 0)
B = (_BX, _BY, 1, _BX * _BY % P)


def _decompress(data: bytes):
    """Decode a compressed point, or None if it does not decode.

    Note what is *not* rejected here: a y coordinate at or above P. `curve25519-dalek` masks
    the sign bit and loads the remainder into field limbs, so such an encoding is accepted and
    behaves as ``y mod P``. Matching that is the point — this file exists to agree with the
    pinned implementation, not to be independently opinionated.
    """
    if len(data) != 32:
        return None
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    y %= P
    x = _recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % P)


def _is_small_order(point) -> bool:
    """True if the point lies in the order-8 torsion subgroup.

    `verify_strict` rejects these for both A and R. A small-order public key verifies
    signatures it never signed, which is precisely the ambiguity a consensus rule cannot
    tolerate.
    """
    return _point_equal(_scalar_mult(point, 8), (0, 1, 1, 0))


def verify_strict(public_key: bytes, message: bytes, signature: bytes) -> bool:
    """Verify under the frozen semantics. Never raises: these bytes come from the wire."""
    try:
        if len(public_key) != 32 or len(signature) != 64:
            return False

        a_point = _decompress(public_key)
        if a_point is None:
            return False
        r_bytes = signature[:32]
        r_point = _decompress(r_bytes)
        if r_point is None:
            return False

        s = int.from_bytes(signature[32:], "little")
        if s >= L:
            # Non-canonical scalar: the same signature would otherwise have many encodings.
            return False

        if _is_small_order(a_point) or _is_small_order(r_point):
            return False

        k = int.from_bytes(
            hashlib.sha512(r_bytes + public_key + message).digest(), "little"
        ) % L

        # Cofactorless: [s]B == R + [k]A.
        left = _scalar_mult(B, s)
        right = _point_add(r_point, _scalar_mult(a_point, k))
        return _point_equal(left, right)
    except Exception:
        return False


def sign(secret_key: bytes, message: bytes) -> bytes:
    """Sign, so the reference executor can build its own fixtures without the Rust side."""
    h = hashlib.sha512(secret_key).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    prefix = h[32:]

    a_point = _scalar_mult(B, a)
    public_key = _compress(a_point)

    r = int.from_bytes(hashlib.sha512(prefix + message).digest(), "little") % L
    r_point = _scalar_mult(B, r)
    r_bytes = _compress(r_point)
    k = int.from_bytes(hashlib.sha512(r_bytes + public_key + message).digest(), "little") % L
    s = (r + k * a) % L
    return r_bytes + s.to_bytes(32, "little")


def public_key(secret_key: bytes) -> bytes:
    h = hashlib.sha512(secret_key).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return _compress(_scalar_mult(B, a))


def _compress(point) -> bytes:
    x, y, z, _ = point
    z_inv = pow(z, P - 2, P)
    x = x * z_inv % P
    y = y * z_inv % P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")
