// A Rust translation of the baseline encoder path in libjpeg-turbo, reduced to the settings Qt
// uses. It is derived from the files below and covered by the IJG License (see licenses/libjpeg-turbo.ijg).
//
// jccolor.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1996, Thomas G. Lane.
//   libjpeg-turbo Modifications:
//   Copyright 2009 Pierre Ossman <ossman@cendio.se> for Cendio AB
//   Copyright (C) 2009-2012, 2015, 2022, 2024-2025, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jcsample.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1996, Thomas G. Lane.
//   Lossless JPEG Modifications:
//   Copyright (C) 1999, Ken Murchison.
//   libjpeg-turbo Modifications:
//   Copyright 2009 Pierre Ossman <ossman@cendio.se> for Cendio AB
//   Copyright (C) 2015, 2019, 2022, 2024-2026, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jfdctint.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1996, Thomas G. Lane.
//   libjpeg-turbo Modifications:
//   Copyright (C) 2015, 2020, 2022, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jcdctmgr.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1994-1996, Thomas G. Lane.
//   libjpeg-turbo Modifications:
//   Copyright (C) 1999-2006, MIYASAKA Masaru.
//   Copyright 2009 Pierre Ossman <ossman@cendio.se> for Cendio AB
//   Copyright (C) 2011, 2014-2015, 2022, 2024-2026, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jchuff.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1997, Thomas G. Lane.
//   Lossless JPEG Modifications:
//   Copyright (C) 1999, Ken Murchison.
//   libjpeg-turbo Modifications:
//   Copyright (C) 2009-2011, 2014-2016, 2018-2026, D. R. Commander.
//   Copyright (C) 2015, Matthieu Darbois.
//   Copyright (C) 2018, Matthias Räncker.
//   Copyright (C) 2020, Arm Limited.
//   Copyright (C) 2022, Felix Hanau.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jcparam.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1998, Thomas G. Lane.
//   Modified 2003-2008 by Guido Vollbeding.
//   Lossless JPEG Modifications:
//   Copyright (C) 1999, Ken Murchison.
//   libjpeg-turbo Modifications:
//   Copyright (C) 2009-2011, 2018, 2023-2024, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jcmarker.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1998, Thomas G. Lane.
//   Modified 2003-2010 by Guido Vollbeding.
//   Lossless JPEG Modifications:
//   Copyright (C) 1999, Ken Murchison.
//   libjpeg-turbo Modifications:
//   Copyright (C) 2010, 2022, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
// jstdhuff.c:
//   This file was part of the Independent JPEG Group's software:
//   Copyright (C) 1991-1998, Thomas G. Lane.
//   libjpeg-turbo Modifications:
//   Copyright (C) 2013, 2022, 2024, D. R. Commander.
//   For conditions of distribution and use, see the accompanying README.ijg
//   file.
//
// Changes: rewritten in Rust; only the 8-bit baseline path is kept (JFIF, YCbCr 4:2:0 with
// h2v2 downsampling, the integer slow DCT, the standard Huffman tables, no optimization).

//! Baseline JPEG encoding as Qt's `QImageWriter` does it through libjpeg(-turbo): JFIF, 4:2:0,
//! the standard tables scaled to a quality, the integer slow DCT, and no Huffman optimization.
//! The output is byte-identical to libjpeg-turbo's.

/// Encode `rgb` (`w` x `h`, 3 bytes a pixel) at `quality`, writing `dpi` in the JFIF header.
pub fn encode(rgb: &[u8], w: usize, h: usize, quality: u32, dpi: (u16, u16)) -> Vec<u8> {
    assert!(w > 0 && h > 0 && rgb.len() >= w * h * 3);
    let q = quant_tables(quality);
    // Component planes, edge-expanded to whole 16 x 16 MCUs.
    let (mw, mh) = (w.div_ceil(16), h.div_ceil(16));
    let (pw, ph) = (mw * 16, mh * 16);
    let (y, cb, cr) = color_convert(rgb, w, h, pw, ph);
    let (cb, cr) = (downsample(&cb, pw, ph, h), downsample(&cr, pw, ph, h));

    let mut out = Vec::with_capacity(w * h / 4);
    out.extend_from_slice(&[0xff, 0xd8]);
    // APP0 JFIF 1.01, dots per inch.
    out.extend_from_slice(&[0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 1]);
    out.extend_from_slice(&dpi.0.to_be_bytes());
    out.extend_from_slice(&dpi.1.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    for (i, t) in q.iter().enumerate() {
        out.extend_from_slice(&[0xff, 0xdb, 0, 67, i as u8]);
        out.extend(ZIGZAG.iter().map(|&k| t[k] as u8));
    }
    out.extend_from_slice(&[0xff, 0xc0, 0, 17, 8]);
    out.extend_from_slice(&(h as u16).to_be_bytes());
    out.extend_from_slice(&(w as u16).to_be_bytes());
    out.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    let tables = [
        (0x00, &DC_LUM_BITS, &DC_LUM_VAL[..]),
        (0x10, &AC_LUM_BITS, &AC_LUM_VAL[..]),
        (0x01, &DC_CHR_BITS, &DC_CHR_VAL[..]),
        (0x11, &AC_CHR_BITS, &AC_CHR_VAL[..]),
    ];
    for (id, bits, vals) in tables {
        out.extend_from_slice(&[0xff, 0xc4]);
        out.extend_from_slice(&((3 + 16 + vals.len()) as u16).to_be_bytes());
        out.push(id);
        out.extend_from_slice(bits);
        out.extend_from_slice(vals);
    }
    out.extend_from_slice(&[0xff, 0xda, 0, 12, 3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);

    let huff = [
        (
            Huff::new(&DC_LUM_BITS, &DC_LUM_VAL),
            Huff::new(&AC_LUM_BITS, &AC_LUM_VAL),
        ),
        (
            Huff::new(&DC_CHR_BITS, &DC_CHR_VAL),
            Huff::new(&AC_CHR_BITS, &AC_CHR_VAL),
        ),
    ];
    let div: Vec<[i32; 64]> = q.iter().map(|t| t.map(|v| v as i32 * 8)).collect();
    // Blocks of each component that lie within the image; others in an MCU are dummies.
    let (yw, yh) = (w.div_ceil(8), h.div_ceil(8));
    let mut bits = Bits::default();
    let mut pred = [0i32; 3];
    for my in 0..mh {
        for mx in 0..mw {
            // jccoefct.c compress_data: a block past the image's right edge repeats the DC of
            // the one before it, a row past its bottom the DC of the last block above.
            let mut blk = [[0i32; 64]; 4];
            for i in 0..4 {
                let (cx, cy) = (mx * 2 + i % 2, my * 2 + i / 2);
                blk[i] = if cy >= yh {
                    dummy(blk[1][0])
                } else if cx >= yw {
                    dummy(blk[i - 1][0])
                } else {
                    quantize(&fdct(&y, pw, cx * 8, cy * 8), &div[0])
                };
                encode_block(&mut bits, &blk[i], &mut pred[0], &huff[0]);
            }
            let cpw = pw / 2;
            for (ci, plane) in [(1, &cb), (2, &cr)] {
                let coef = quantize(&fdct(plane, cpw, mx * 8, my * 8), &div[1]);
                encode_block(&mut bits, &coef, &mut pred[ci], &huff[1]);
            }
        }
    }
    bits.flush();
    out.extend_from_slice(&bits.out);
    out.extend_from_slice(&[0xff, 0xd9]);
    out
}

fn dummy(dc: i32) -> [i32; 64] {
    let mut b = [0; 64];
    b[0] = dc;
    b
}

/// jpeg_quality_scaling + jpeg_add_quant_table (baseline).
fn quant_tables(quality: u32) -> [[u16; 64]; 2] {
    let quality = quality.clamp(1, 100);
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - quality * 2
    };
    [LUM_Q, CHR_Q].map(|base| base.map(|b| ((b as u32 * scale + 50) / 100).clamp(1, 255) as u16))
}

/// jccolor.c rgb_ycc_convert, with the rows and columns past the image replicated
/// (jcsample.c expand_right_edge, jcprepct.c expand_bottom_edge).
fn color_convert(
    rgb: &[u8],
    w: usize,
    h: usize,
    pw: usize,
    ph: usize,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    const fn fix(x: f64) -> i32 {
        (x * 65536.0 + 0.5) as i32
    }
    const HALF: i32 = 1 << 15;
    const OFF: i32 = 128 << 16;
    let mut y = vec![0u8; pw * ph];
    let mut cb = vec![0u8; pw * ph];
    let mut cr = vec![0u8; pw * ph];
    for row in 0..ph {
        let sr = row.min(h - 1);
        for col in 0..pw {
            let sc = col.min(w - 1);
            let p = &rgb[(sr * w + sc) * 3..];
            let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
            let i = row * pw + col;
            y[i] = ((fix(0.29900) * r + fix(0.58700) * g + fix(0.11400) * b + HALF) >> 16) as u8;
            cb[i] = ((-fix(0.16874) * r - fix(0.33126) * g + fix(0.5) * b + OFF + HALF - 1) >> 16)
                as u8;
            cr[i] =
                ((fix(0.5) * r - fix(0.41869) * g - fix(0.08131) * b + OFF + HALF - 1) >> 16) as u8;
        }
    }
    (y, cb, cr)
}

/// jcsample.c h2v2_downsample: 2 x 2 averages with the bias alternating 1, 2 along each row.
/// Only the rows of the `h` image rows (with the last repeated to an even count) are
/// downsampled; below them jcprepct.c repeats the last downsampled row.
fn downsample(p: &[u8], pw: usize, ph: usize, h: usize) -> Vec<u8> {
    let (ow, oh) = (pw / 2, ph / 2);
    let mut o = vec![0u8; ow * oh];
    let rows = h.div_ceil(2);
    for r in 0..rows {
        let (a, b) = (&p[2 * r * pw..], &p[(2 * r + 1) * pw..]);
        let mut bias = 1;
        for c in 0..ow {
            let s = a[2 * c] as u32 + a[2 * c + 1] as u32 + b[2 * c] as u32 + b[2 * c + 1] as u32;
            o[r * ow + c] = ((s + bias) >> 2) as u8;
            bias ^= 3;
        }
    }
    for r in rows..oh {
        o.copy_within((rows - 1) * ow..rows * ow, r * ow);
    }
    o
}

/// jfdctint.c jpeg_fdct_islow on the 8 x 8 block at (x, y), samples centered on 0.
fn fdct(p: &[u8], stride: usize, x: usize, y: usize) -> [i32; 64] {
    const CB: i32 = 13;
    const P1: i32 = 2;
    const F0_298: i32 = 2446;
    const F0_390: i32 = 3196;
    const F0_541: i32 = 4433;
    const F0_765: i32 = 6270;
    const F0_899: i32 = 7373;
    const F1_175: i32 = 9633;
    const F1_501: i32 = 12299;
    const F1_847: i32 = 15137;
    const F1_961: i32 = 16069;
    const F2_053: i32 = 16819;
    const F2_562: i32 = 20995;
    const F3_072: i32 = 25172;
    let desc = |v: i32, n: i32| (v + (1 << (n - 1))) >> n;
    let mut d = [0i32; 64];
    for r in 0..8 {
        for c in 0..8 {
            d[r * 8 + c] = p[(y + r) * stride + x + c] as i32 - 128;
        }
    }
    // Pass 1: rows.
    for r in 0..8 {
        let b = &mut d[r * 8..r * 8 + 8];
        let tmp0 = b[0] + b[7];
        let tmp7 = b[0] - b[7];
        let tmp1 = b[1] + b[6];
        let tmp6 = b[1] - b[6];
        let tmp2 = b[2] + b[5];
        let tmp5 = b[2] - b[5];
        let tmp3 = b[3] + b[4];
        let tmp4 = b[3] - b[4];
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        b[0] = (tmp10 + tmp11) << P1;
        b[4] = (tmp10 - tmp11) << P1;
        let z1 = (tmp12 + tmp13) * F0_541;
        b[2] = desc(z1 + tmp13 * F0_765, CB - P1);
        b[6] = desc(z1 + tmp12 * -F1_847, CB - P1);
        let (z1, z2, z3, z4) = (tmp4 + tmp7, tmp5 + tmp6, tmp4 + tmp6, tmp5 + tmp7);
        let z5 = (z3 + z4) * F1_175;
        let tmp4 = tmp4 * F0_298;
        let tmp5 = tmp5 * F2_053;
        let tmp6 = tmp6 * F3_072;
        let tmp7 = tmp7 * F1_501;
        let z1 = z1 * -F0_899;
        let z2 = z2 * -F2_562;
        let z3 = z3 * -F1_961 + z5;
        let z4 = z4 * -F0_390 + z5;
        b[7] = desc(tmp4 + z1 + z3, CB - P1);
        b[5] = desc(tmp5 + z2 + z4, CB - P1);
        b[3] = desc(tmp6 + z2 + z3, CB - P1);
        b[1] = desc(tmp7 + z1 + z4, CB - P1);
    }
    // Pass 2: columns.
    for c in 0..8 {
        let g = |r: usize| d[r * 8 + c];
        let tmp0 = g(0) + g(7);
        let tmp7 = g(0) - g(7);
        let tmp1 = g(1) + g(6);
        let tmp6 = g(1) - g(6);
        let tmp2 = g(2) + g(5);
        let tmp5 = g(2) - g(5);
        let tmp3 = g(3) + g(4);
        let tmp4 = g(3) - g(4);
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let mut o = [0i32; 8];
        o[0] = desc(tmp10 + tmp11, P1);
        o[4] = desc(tmp10 - tmp11, P1);
        let z1 = (tmp12 + tmp13) * F0_541;
        o[2] = desc(z1 + tmp13 * F0_765, CB + P1);
        o[6] = desc(z1 + tmp12 * -F1_847, CB + P1);
        let (z1, z2, z3, z4) = (tmp4 + tmp7, tmp5 + tmp6, tmp4 + tmp6, tmp5 + tmp7);
        let z5 = (z3 + z4) * F1_175;
        let tmp4 = tmp4 * F0_298;
        let tmp5 = tmp5 * F2_053;
        let tmp6 = tmp6 * F3_072;
        let tmp7 = tmp7 * F1_501;
        let z1 = z1 * -F0_899;
        let z2 = z2 * -F2_562;
        let z3 = z3 * -F1_961 + z5;
        let z4 = z4 * -F0_390 + z5;
        o[7] = desc(tmp4 + z1 + z3, CB + P1);
        o[5] = desc(tmp5 + z2 + z4, CB + P1);
        o[3] = desc(tmp6 + z2 + z3, CB + P1);
        o[1] = desc(tmp7 + z1 + z4, CB + P1);
        for r in 0..8 {
            d[r * 8 + c] = o[r];
        }
    }
    d
}

/// jcdctmgr.c quantize: rounded division of the magnitude, in natural order.
fn quantize(d: &[i32; 64], div: &[i32; 64]) -> [i32; 64] {
    let mut o = [0; 64];
    for i in 0..64 {
        let (v, q) = (d[i], div[i]);
        let m = (v.abs() + (q >> 1)) / q;
        o[i] = if v < 0 { -m } else { m };
    }
    o
}

struct Huff {
    code: [u16; 256],
    size: [u8; 256],
}

impl Huff {
    /// jchuff.c jpeg_make_c_derived_tbl.
    fn new(bits: &[u8; 16], vals: &[u8]) -> Huff {
        let mut h = Huff {
            code: [0; 256],
            size: [0; 256],
        };
        let (mut code, mut k) = (0u16, 0);
        for (l, &n) in bits.iter().enumerate() {
            for _ in 0..n {
                h.code[vals[k] as usize] = code;
                h.size[vals[k] as usize] = l as u8 + 1;
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        h
    }
}

#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, code: u32, size: u32) {
        self.acc = (self.acc << size) | (code & ((1 << size) - 1));
        self.n += size;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xff {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }

    /// Pad the last byte with 1 bits.
    fn flush(&mut self) {
        if self.n > 0 {
            self.put(0x7f, 8 - self.n);
        }
    }
}

fn magnitude(v: i32) -> (u32, u32) {
    let a = v.unsigned_abs();
    let n = 32 - a.leading_zeros();
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (n, bits & ((1u32 << n) - 1))
}

/// jchuff.c encode_one_block.
fn encode_block(b: &mut Bits, coef: &[i32; 64], pred: &mut i32, (dc, ac): &(Huff, Huff)) {
    let (n, v) = magnitude(coef[0] - *pred);
    *pred = coef[0];
    b.put(dc.code[n as usize] as u32, dc.size[n as usize] as u32);
    if n > 0 {
        b.put(v, n);
    }
    let mut run = 0;
    for &k in &ZIGZAG[1..] {
        let c = coef[k];
        if c == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            b.put(ac.code[0xf0] as u32, ac.size[0xf0] as u32);
            run -= 16;
        }
        let (n, v) = magnitude(c);
        let s = (run << 4) | n as usize;
        b.put(ac.code[s] as u32, ac.size[s] as u32);
        b.put(v, n);
        run = 0;
    }
    if run > 0 {
        b.put(ac.code[0] as u32, ac.size[0] as u32);
    }
}

/// Natural-order index of each zigzag position.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const LUM_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const CHR_Q: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUM_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_LUM_VAL: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const DC_CHR_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_CHR_VAL: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUM_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUM_VAL: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_CHR_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHR_VAL: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// An RGB test pattern with detail in every channel.
    fn pattern(w: usize, h: usize) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                [
                    ((x * 7 + y * 3) ^ (x * y)) as u8,
                    (x * x + y * 5) as u8,
                    (y * 11 + x) as u8,
                ]
            })
            .collect()
    }

    /// libjpeg-turbo's output (through Pillow) at Qt's settings, named `{w}x{h}_q{quality}.jpg`.
    #[test]
    fn matches_libjpeg_turbo() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/jpeg");
        let mut cases = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap();
            let (size, q) = name.split_once("_q").unwrap();
            let (w, h) = size.split_once('x').unwrap();
            let (w, h, q) = (w.parse().unwrap(), h.parse().unwrap(), q.parse().unwrap());
            let want = std::fs::read(&path).unwrap();
            assert!(encode(&pattern(w, h), w, h, q, (96, 96)) == want, "{name}");
            cases += 1;
        }
        assert!(cases > 0);
    }
}
