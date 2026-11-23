//! Shared benchmark and differential test fixtures.
//! The synthetic LUTs follow imazen/zenpixels commit a08a4404.

pub fn display_p3() -> Vec<u8> {
    use lcms::types::{CiexyY, CiexyYTriple};
    let xy = |x, y| CiexyY { x, y, Y: 1.0 };
    let trc = lcms::gamma::build_parametric_tone_curve(
        None,
        4,
        &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
    )
    .unwrap();
    let white = xy(0.3127, 0.3290);
    let primaries = CiexyYTriple {
        Red: xy(0.680, 0.320),
        Green: xy(0.265, 0.690),
        Blue: xy(0.150, 0.060),
    };
    let mut profile = lcms::virt::create_rgb_profile(
        None,
        Some(&white),
        Some(&primaries),
        Some([&trc, &trc, &trc]),
    )
    .unwrap();
    lcms::profile::save_profile_to_mem(&mut profile).unwrap()
}

pub fn adobe_rgb() -> Vec<u8> {
    use lcms::types::{CiexyY, CiexyYTriple};
    let xy = |x, y| CiexyY { x, y, Y: 1.0 };
    let trc = lcms::gamma::build_gamma(None, 563.0 / 256.0).unwrap();
    let white = xy(0.3127, 0.3290);
    let primaries = CiexyYTriple {
        Red: xy(0.640, 0.330),
        Green: xy(0.210, 0.710),
        Blue: xy(0.150, 0.060),
    };
    let mut profile = lcms::virt::create_rgb_profile(
        None,
        Some(&white),
        Some(&primaries),
        Some([&trc, &trc, &trc]),
    )
    .unwrap();
    lcms::profile::save_profile_to_mem(&mut profile).unwrap()
}

pub fn synthetic_cmyk_icc() -> Vec<u8> {
    const GRID: u32 = 9;
    let node_lab = |c: f64, m: f64, y: f64, k: f64| -> [u16; 3] {
        let lit = 1.0 - k;
        let l = 100.0 * lit * (1.0 - 0.3 * c - 0.4 * m - 0.1 * y);
        let a = 60.0 * lit * (m - c);
        let b = 70.0 * lit * (y - 0.5 * (c + m));
        [
            (l * 652.8).round() as u16,
            ((a + 128.0) * 256.0).round() as u16,
            ((b + 128.0) * 256.0).round() as u16,
        ]
    };
    lut_profile(*b"prtr", *b"CMYK", 4, GRID, |node| {
        let ink = |place: u32| f64::from(node / GRID.pow(place) % GRID) / f64::from(GRID - 1);
        node_lab(ink(3), ink(2), ink(1), ink(0))
    })
}

pub fn synthetic_rgb_icc() -> Vec<u8> {
    const NODES: u32 = 17;
    const M: [[f64; 3]; 3] = [
        [0.4360, 0.3851, 0.1431],
        [0.2225, 0.7169, 0.0606],
        [0.0139, 0.0971, 0.7139],
    ];
    const WHITE: [f64; 3] = [0.9642, 1.0, 0.8249];
    let lab_f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let decode = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    lut_profile(*b"spac", *b"RGB ", 3, NODES, |node| {
        let ch =
            |place: u32| decode(f64::from(node / NODES.pow(place) % NODES) / f64::from(NODES - 1));
        let rgb = [ch(2), ch(1), ch(0)];
        let fxyz = [0, 1, 2].map(|i| {
            let v = M[i][0] * rgb[0] + M[i][1] * rgb[1] + M[i][2] * rgb[2];
            lab_f(v / WHITE[i])
        });
        let l = 116.0 * fxyz[1] - 16.0;
        let a = 500.0 * (fxyz[0] - fxyz[1]);
        let b = 200.0 * (fxyz[1] - fxyz[2]);
        [l * 652.8, (a + 128.0) * 256.0, (b + 128.0) * 256.0]
            .map(|v| v.round().clamp(0.0, 65535.0) as u16)
    })
}

fn lut_profile(
    class: [u8; 4],
    space: [u8; 4],
    inputs: u8,
    grid: u32,
    node: impl Fn(u32) -> [u16; 3],
) -> Vec<u8> {
    let mut lut = b"mft2\0\0\0\0".to_vec();
    lut.extend([inputs, 3, grid as u8, 0]);
    for row in 0..3 {
        for col in 0..3 {
            lut.extend((if row == col { 65536u32 } else { 0 }).to_be_bytes());
        }
    }
    lut.extend([0, 2, 0, 2]);
    for _ in 0..inputs {
        lut.extend([0, 0, 255, 255]);
    }
    for i in 0..grid.pow(u32::from(inputs)) {
        for sample in node(i) {
            lut.extend(sample.to_be_bytes());
        }
    }
    for _ in 0..3 {
        lut.extend([0, 0, 255, 255]);
    }

    const TAG_START: usize = 144;
    let mut icc = vec![0; 128];
    icc[0..4].copy_from_slice(&((TAG_START + lut.len()) as u32).to_be_bytes());
    icc[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    icc[12..16].copy_from_slice(&class);
    icc[16..20].copy_from_slice(&space);
    icc[20..24].copy_from_slice(b"Lab ");
    icc[36..40].copy_from_slice(b"acsp");
    for (i, v) in [0xF6D6u32, 0x10000, 0xD32D].into_iter().enumerate() {
        icc[68 + i * 4..72 + i * 4].copy_from_slice(&v.to_be_bytes());
    }
    icc.extend(1u32.to_be_bytes());
    icc.extend(*b"A2B0");
    icc.extend((TAG_START as u32).to_be_bytes());
    icc.extend((lut.len() as u32).to_be_bytes());
    icc.extend(lut);
    icc
}
