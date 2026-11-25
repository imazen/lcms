//! Prepared three-channel CLUTs for packed RGB8 and RGB16 plans.

use crate::intrp::{InterpParams, InterpTable, Interpolation, lanes_u64, tetrahedral_interp16};
use crate::opt::{ClutOnly, Prelin8Data};
use magetypes::simd::generic::i32x4;

#[derive(Clone, Copy)]
struct Axis {
    base: u32,
    // The fraction orders the axes. The low half carries the grid step
    // (zero for a collapsed edge), so the same sort orders the steps.
    rank: u32,
}

pub(crate) struct PackedRgbClut {
    table: Box<[u64]>,
    bytes: Option<Box<[[Axis; 256]; 3]>>,
}

impl PackedRgbClut {
    pub(crate) fn direct(clut: &mut ClutOnly, byte_input: bool) -> Option<Self> {
        let p = &clut.params16;
        if p.n_inputs != 3 || p.n_outputs != 3 || p.foreign.is_some() || !is_tetrahedral(p) {
            return None;
        }
        if !byte_input && p.domain[..3] != [32; 3] {
            return None;
        }
        let tab = clut.clut.tab16.as_ref()?;
        // The resampler snapshots the same table twice. Share the
        // allocation for the generic evaluator of this frozen plan.
        match &p.table {
            InterpTable::U16Rc(words) if words.as_ref() == tab.as_ref() => {}
            _ => return None,
        }
        let (mut out, steps) = Self::new(tab, p.domain, p.opta)?;
        if byte_input {
            let axes = std::array::from_fn(|ch| {
                std::array::from_fn(|code| {
                    let code = crate::pack::f8to16(code as u8);
                    let fixed =
                        crate::fixed::to_fixed_domain(i32::from(code) * p.domain[ch] as i32);
                    Axis::new(fixed as u32, steps[ch], code == u16::MAX)
                })
            });
            out.bytes = Some(Box::new(axes));
        }
        clut.params16.table = InterpTable::U16Rc(tab.clone());
        Some(out)
    }

    pub(crate) fn prelinearized(p: &Prelin8Data) -> Option<Self> {
        if p.clut.n_inputs != 3 || p.clut.n_outputs != 3 || p.clut.foreign.is_some() {
            return None;
        }
        let (mut out, steps) = Self::new(p.clut.tab16.as_ref()?, p.clut.domain, p.clut.opta)?;
        let bases = [&p.x0, &p.y0, &p.z0];
        let fractions = [&p.rx, &p.ry, &p.rz];
        let axes = std::array::from_fn(|ch| {
            std::array::from_fn(|code| {
                let fraction = u32::from(fractions[ch][code]);
                let step = if fraction == 0 { 0 } else { steps[ch] };
                Axis {
                    base: bases[ch][code] / 3,
                    rank: (fraction << 16) | step,
                }
            })
        });
        out.bytes = Some(Box::new(axes));
        Some(out)
    }

    fn new(
        tab: &[u16],
        [r, g, b, ..]: [u32; crate::intrp::MAX_INPUT_DIMENSIONS],
        [bz, gy, rx, ..]: [u32; crate::intrp::MAX_INPUT_DIMENSIONS],
    ) -> Option<(Self, [u32; 3])> {
        // Keep storage bounded to the normal-resolution grid. For these
        // sizes the sum of the three node steps also fits in 16 bits.
        if [r, g, b].iter().any(|d| !(1..=32).contains(d)) {
            return None;
        }
        let steps = [(g + 1) * (b + 1), b + 1, 1];
        if [rx, gy, bz] != steps.map(|step| step * 3)
            || ((r + 1) * steps[0] * 3) as usize != tab.len()
        {
            return None;
        }
        let mut table = bytemuck::try_zeroed_vec::<u64>(tab.len() / 3).ok()?;
        for (&[r, g, b], node) in tab.as_chunks::<3>().0.iter().zip(&mut table) {
            *node = u64::from(r) | (u64::from(g) << 16) | (u64::from(b) << 32);
        }
        Some((
            Self {
                table: table.into_boxed_slice(),
                bytes: None,
            },
            steps,
        ))
    }

    #[inline(always)]
    pub(crate) fn eval8(
        &self,
        token: archmage::X64V3Token,
        [ri, gi, bi]: [u8; 3],
    ) -> ([u16; 3], [u8; 3]) {
        let [r, g, b] = self.bytes.as_deref().unwrap();
        let result = self.eval(token, [r[ri as usize], g[gi as usize], b[bi as usize]]);
        let words = result & i32x4::splat_t(token, 0xffff);
        // Apply f16to8 across the output lanes, using a wrapping multiply
        // and a logical shift to take the unsigned high byte.
        let bytes = (words * i32x4::splat_t(token, 65281) + i32x4::splat_t(token, 8388608))
            .shr_logical::<24>();
        let [r, g, b, _] = result.to_array();
        let [rb, gb, bb, _] = bytes.to_array();
        (
            [r as u16, g as u16, b as u16],
            [rb as u8, gb as u8, bb as u8],
        )
    }

    #[inline(always)]
    pub(crate) fn eval16(&self, token: archmage::X64V3Token, [r, g, b]: [u16; 3]) -> [u16; 3] {
        let axis = |code: u16, step| {
            let x = u32::from(code);
            // On the 33³ grid, the domain is 32. For every u16 code,
            // (32 * x + 32767) / 65535 equals (x + 1024) >> 11.
            let fixed = (x << 5) + ((x + 1024) >> 11);
            Axis::new(fixed, step, code == u16::MAX)
        };
        let [r, g, b, _] = self
            .eval(token, [axis(r, 33 * 33), axis(g, 33), axis(b, 1)])
            .to_array();
        [r as u16, g as u16, b as u16]
    }

    #[inline(always)]
    fn eval(
        &self,
        token: archmage::X64V3Token,
        [r, g, b]: [Axis; 3],
    ) -> i32x4<archmage::X64V3Token> {
        let high = r.rank.max(g.rank).max(b.rank);
        let low = r.rank.min(g.rank).min(b.rank);
        let total = r.rank.wrapping_add(g.rank).wrapping_add(b.rank);
        let mid = total.wrapping_sub(high).wrapping_sub(low);
        let base = r.base + g.base + b.base;
        let corner = |offset: u32| {
            i32x4::from_array_t(
                token,
                lanes_u64(self.table[(base + (offset & 0xffff)) as usize]),
            )
        };
        let start = corner(0);
        let [a, b, c] = [high, total.wrapping_sub(low), total].map(corner);
        // Ties can take either edge first. Their products telescope in
        // wrapping i32 arithmetic, before applying the original rounding.
        let [high, mid, low] = [high, mid, low].map(|v| i32x4::splat_t(token, (v >> 16) as i32));
        let rest =
            (a - start) * high + (b - a) * mid + (c - b) * low + i32x4::splat_t(token, 0x8001);
        start + (rest + rest.shr_arithmetic::<16>()).shr_arithmetic::<16>()
    }
}

impl Axis {
    #[inline(always)]
    fn new(fixed: u32, step: u32, at_end: bool) -> Self {
        let edge = if at_end { 0 } else { step };
        Self {
            base: (fixed >> 16) * step,
            rank: (fixed << 16) | edge,
        }
    }
}

fn is_tetrahedral(p: &InterpParams) -> bool {
    matches!(
        p.interpolation, Interpolation::Lerp16(f)
            if std::ptr::fn_addr_eq(f, tetrahedral_interp16 as crate::intrp::InterpFn16)
    )
}
