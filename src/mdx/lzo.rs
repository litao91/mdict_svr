//! LZO1X decompression.
//!
//! Implements the public LZO1X stream format (the byte-level instruction encoding
//! documented for LZO, e.g. the "LZO" description in the Linux kernel docs). MDict
//! v1.x dictionaries commonly store their key and record blocks LZO1X-compressed;
//! the decompressed length is known from the block index, so we allocate exactly
//! that and require the stream to consume all input and fill all output.
//!
//! The stream is a sequence of instructions. The first byte decides whether the
//! stream opens with a literal run; thereafter each instruction is one of:
//!   * `0..=15`  — a literal run (state A) or a short M1 match (state B/C),
//!   * `16..=31` — an M4 match (distance base 16384; also the end-of-stream mark),
//!   * `32..=63` — an M3 match,
//!   * `64..=255` — an M2 match.
//! After a match, the low 2 bits of the instruction give 0–3 trailing literal
//! bytes and select the next state.

use crate::error::{Error, Result};

/// Decompress an LZO1X stream into exactly `dst_len` bytes.
pub fn decompress(src: &[u8], dst_len: usize) -> Result<Vec<u8>> {
    let mut dst = vec![0u8; dst_len];
    let mut ip = 0usize;
    let mut op = 0usize;

    // State after the previous instruction: 0 = A (expect literal run), 1 = B, 2 = C.
    let mut state: u8;

    macro_rules! need {
        ($n:expr) => {
            if ip + ($n) > src.len() {
                return Err(Error::Decompress("lzo: unexpected end of input".into()));
            }
        };
    }
    macro_rules! need_out {
        ($n:expr) => {
            if op + ($n) > dst_len {
                return Err(Error::Decompress("lzo: output overflow".into()));
            }
        };
    }

    // Opening instruction.
    need!(1);
    let first = src[ip];
    if first > 17 {
        ip += 1;
        let lit_len = first as usize - 17;
        need!(lit_len);
        need_out!(lit_len);
        dst[op..op + lit_len].copy_from_slice(&src[ip..ip + lit_len]);
        ip += lit_len;
        op += lit_len;
        state = if first <= 20 { 1 } else { 2 };
    } else {
        state = 0;
    }

    loop {
        need!(1);
        let insn = src[ip];
        ip += 1;

        let match_len: usize;
        let match_dist: usize;
        let lit_insn: u8;

        if insn < 16 {
            if state == 0 {
                // Literal run.
                let lit_len = if insn == 0 {
                    let start = ip;
                    loop {
                        need!(1);
                        if src[ip] != 0 {
                            break;
                        }
                        ip += 1;
                    }
                    let count = ip - start;
                    let v = src[ip] as usize;
                    ip += 1;
                    count * 255 + v + 18
                } else {
                    insn as usize + 3
                };
                need!(lit_len);
                need_out!(lit_len);
                dst[op..op + lit_len].copy_from_slice(&src[ip..ip + lit_len]);
                ip += lit_len;
                op += lit_len;
                state = 2; // C
                continue;
            }
            // Short M1 match: state B -> len 2, dist base 1; state C -> len 3, dist base 2049.
            let (ml, dist_base) = if state == 1 { (2usize, 1usize) } else { (3usize, 2049usize) };
            need!(1);
            match_len = ml;
            match_dist = ((src[ip] as usize) << 2) + ((insn >> 2) as usize) + dist_base;
            ip += 1;
            lit_insn = insn;
        } else if insn < 32 {
            // M4 match (distance base 16384), or the end-of-stream marker.
            let ml = if insn & 7 == 0 {
                let start = ip;
                loop {
                    need!(1);
                    if src[ip] != 0 {
                        break;
                    }
                    ip += 1;
                }
                let count = ip - start;
                let v = src[ip] as usize;
                ip += 1;
                count * 255 + v + 9
            } else {
                (insn & 7) as usize + 2
            };
            need!(2);
            let dist = ((((insn & 8) >> 3) as usize) << 14)
                + ((src[ip + 1] as usize) << 6)
                + ((src[ip] >> 2) as usize)
                + 16384;
            let sub = src[ip];
            ip += 2;
            if dist == 16384 {
                break; // end of stream
            }
            match_len = ml;
            match_dist = dist;
            lit_insn = sub;
        } else if insn < 64 {
            // M3 match.
            let ml = if insn & 0x1f == 0 {
                let start = ip;
                loop {
                    need!(1);
                    if src[ip] != 0 {
                        break;
                    }
                    ip += 1;
                }
                let count = ip - start;
                let v = src[ip] as usize;
                ip += 1;
                count * 255 + v + 33
            } else {
                (insn & 0x1f) as usize + 2
            };
            need!(2);
            let dist = ((src[ip + 1] as usize) << 6) + ((src[ip] >> 2) as usize) + 1;
            let sub = src[ip];
            ip += 2;
            match_len = ml;
            match_dist = dist;
            lit_insn = sub;
        } else {
            // M2 match.
            let ml = if insn < 128 {
                if insn & 0x20 != 0 {
                    4
                } else {
                    3
                }
            } else {
                ((insn & 0x60) >> 5) as usize + 5
            };
            need!(1);
            let dist = ((src[ip] as usize) << 3) + (((insn & 0x1c) >> 2) as usize) + 1;
            ip += 1;
            match_len = ml;
            match_dist = dist;
            lit_insn = insn;
        }

        // Copy the match. Byte-by-byte so overlapping matches (dist < len) repeat
        // correctly, as LZO requires.
        if match_dist == 0 || match_dist > op {
            return Err(Error::Decompress("lzo: invalid match distance".into()));
        }
        need_out!(match_len);
        let mpos = op - match_dist;
        for k in 0..match_len {
            dst[op + k] = dst[mpos + k];
        }
        op += match_len;

        // Trailing literals (0–3), which also set the next state.
        let lit_len = (lit_insn & 3) as usize;
        if lit_len == 0 {
            state = 0; // A
        } else {
            need!(lit_len);
            need_out!(lit_len);
            dst[op..op + lit_len].copy_from_slice(&src[ip..ip + lit_len]);
            ip += lit_len;
            op += lit_len;
            state = 1; // B
        }
    }

    if ip != src.len() {
        return Err(Error::Decompress(format!(
            "lzo: consumed {ip} of {} input bytes",
            src.len()
        )));
    }
    if op != dst_len {
        return Err(Error::Decompress(format!(
            "lzo: produced {op} of {dst_len} expected bytes"
        )));
    }
    Ok(dst)
}
