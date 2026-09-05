import json, re, struct, sys

MOD = 268435456
M32 = 4294967296
T = json.load(open('/home/user/work/tables.json'))
K = {int(a): b for a, b in T['K'].items()}
Ytab = {k.encode('latin1'): v.encode('latin1') for k, v in T['Y'].items()}
XF = {int(a): b for a, b in T['XF'].items()}

IC = (63947346 * 12601 + 58084353) % MOD
wI = (58134956 * 113 + 48872959) % MOD
Ik = (10871310 * 7319 + 182149595) % MOD
gL = (243054068 * 2375 + 163530629) % MOD
SM = (72703548 * 7779 + 94491131) % MOD
L = (207046853 + 265920286) % MOD


def unescape(au):
    LV = bytearray()
    i = 0
    while i < len(au):
        if au[i] == 89:
            LV += Ytab.get(bytes(au[i:i + 5]), b'')
            i += 5
        else:
            LV.append(au[i]); i += 1
    return bytes(LV)


def b94(LV):
    B = bytearray()
    for A in range(0, len(LV), 5):
        b = 0
        for t in range(5):
            b = b * 94 + K[LV[A + t]]
        for t in range(4):
            B.append(b % 256); b //= 256
    return bytes(B)


def m_decode(au):
    LV = unescape(au)
    B = b94(LV)
    j = B[0] | B[1] << 8 | B[2] << 16 | B[3] << 24
    return B[4:4 + j], j, len(LV), len(B)


def varint(v, P):
    b0 = v[P]
    if b0 < 128: return b0, P + 1
    b1 = v[P + 1]
    if b1 < 128: return b0 - 128 + b1 * 128, P + 2
    b2 = v[P + 2]
    if b2 < 128:
        r = b0 - 128 + (b1 - 128) * 128 + b2 * 16384
        return (r - M32 if r >= 2147483648 else r), P + 3
    b3 = v[P + 3]
    r = b0 - 128 + (b1 - 128) * 128 + (b2 - 128) * 16384 + b3 * 2097152
    return (r - M32 if r >= 2147483648 else r), P + 4


def read_varint_array(v, P, n):
    out = []
    for _ in range(n):
        x, P = varint(v, P)
        out.append(x)
    return out, P


def parse_proto(blob, hA=1, verbose=True):
    raw, j, nLV, nB = m_decode(blob)
    if verbose:
        print('unescaped=%d  b94out=%d  declared=%d  body=%d' % (nLV, nB, j, len(raw)))
    dU = (SM + (hA - 1) * L) % MOD
    dec = bytearray()
    for c in raw:
        dU = (Ik * dU + gL) % MOD
        dec.append((c - dU % 256) % 256)
    v = bytes(dec)
    if verbose:
        print('section tags:', [v[i] for i in range(min(len(v), 40))])
    ON, zM, Wp, ZQ, Rs, re_ = 118, 221, 237, 86, 82, 230
    P = 0; d = 0; Bd = 0; li = 1
    pr = {}
    zu = None
    while li <= 2:
        if li == 1:
            dW = v[P]; P += 1
            if dW == ON:
                pr['nregs'], P = varint(v, P)
                pr['nparams'], P = varint(v, P)
                pr['vararg'], P = varint(v, P)
                d += 1
            elif dW == zM:
                zu, P = varint(v, P)
                pr['cseed'] = zu
                d += 1
            elif dW == Wp:
                n, P = varint(v, P)
                pr['upsrc'], P = read_varint_array(v, P, n)
                d += 1
            elif dW == ZQ:
                X, P = varint(v, P)
                FF, P = varint(v, P)
                pr['nconst'] = X; pr['clen'] = FF
                Bd = P; P += FF
                d += 1
            elif dW == Rs:
                n, P = varint(v, P)
                pr['S'], P = read_varint_array(v, P, n)
                d += 1
            elif dW == re_:
                MT, P = varint(v, P)
                pr.setdefault('skipped', []).append((P, MT))
                P += MT
            else:
                Or, P = varint(v, P)
                pr['ninst'] = Or
                pr['W'] = list(v[P:P + Or]); P += Or
                dm = P
                pr['SA'], P = read_varint_array(v, P, Or)
                pr['SB'], P = read_varint_array(v, P, Or)
                pr['SC'], P = read_varint_array(v, P, Or)
                pr['SD'], P = read_varint_array(v, P, Or)
                uV = P; P = dm
                W_ = 0
                for _ in range(4):
                    for _q in range(Or):
                        x, P = varint(v, P)
                        W_ = (W_ + x) % M32
                pr['ck'] = W_
                P = uV
                d += 1
            if d >= 6: li = 2
        else:
            P = Bd
            state = {'zu': zu}

            def uw():
                state['zu'] = (IC * state['zu'] + wI) % MOD
                c = (v[P_loc[0]] - state['zu'] % 256) % 256
                P_loc[0] += 1
                return c
            P_loc = [P]
            IB = {}
            for gh in range(1, pr['nconst'] + 1):
                tag = uw()
                if tag == 0:
                    IB[gh] = None
                elif tag == 1:
                    IB[gh] = (uw() == 1)
                elif tag == 3:
                    lo = uw(); hi = uw()
                    ln = lo + hi * 256
                    s = bytes([uw() for _ in range(ln)])
                    IB[gh] = s.decode('latin1')
                else:
                    Jo = 0; EJ = 1
                    while True:
                        Ih = uw()
                        if Ih < 128:
                            Jo += Ih * EJ; break
                        Jo += (Ih - 128) * EJ; EJ *= 128
                    Gl = 0; mX = 1
                    while True:
                        gp = uw()
                        if gp < 128:
                            Gl += gp * mX; break
                        Gl += (gp - 128) * mX; mX *= 128
                    Ut = Gl // 2 - 2048
                    val = Jo * (2.0 ** Ut)
                    if Gl % 2 == 1: val = -val
                    IB[gh] = val
            P = P_loc[0]
            pr['C'] = IB
            ok = (P == Bd + pr['clen'])
            pr['clen_ok'] = ok
            if verbose:
                print('const-pool consumed %d/%d bytes  OK=%s' % (P - Bd, pr['clen'], ok))
            li = 3
    pr['total'] = len(v)
    pr['P_end'] = P
    return pr, v
