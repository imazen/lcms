//! Differential test: `lcms::intrp` vs `cmsintrp.c`.
//!
//! Builds matching `cmsInterpParams`/`InterpParams` on both sides, then feeds
//! identical inputs — dense randoms plus edge values (0x0000, 0xffff, exact
//! grid points, NaN for floats) — and compares outputs bit-for-bit.

use lcms::intrp::*;
use lcms_cref as c;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn rand_table16(s: &mut u64, len: usize) -> Vec<u16> {
    (0..len).map(|_| rng(s) as u16).collect()
}
fn rand_tablef(s: &mut u64, len: usize) -> Vec<f32> {
    (0..len)
        .map(|_| (rng(s) as f32 / (1u64 << 53) as f32) * 4.0 - 1.5)
        .collect()
}

/// C side: build params, call the bound kernel.
fn c_eval16(
    samples: &[u32],
    n_in: u32,
    n_out: u32,
    table: &[u16],
    flags: u32,
    input: &[u16],
    output: &mut [u16],
) -> bool {
    unsafe {
        let p = c::_cmsComputeInterpParamsEx(
            std::ptr::null_mut(),
            samples.as_ptr(),
            n_in,
            n_out,
            table.as_ptr() as *const std::ffi::c_void,
            flags,
        );
        if p.is_null() {
            return false;
        }
        let f = (*p).interpolation.lerp16;
        match f {
            Some(f) => f(input.as_ptr(), output.as_mut_ptr(), p),
            None => {
                c::_cmsFreeInterpParams(p);
                return false;
            }
        }
        c::_cmsFreeInterpParams(p);
        true
    }
}

fn c_eval_float(
    samples: &[u32],
    n_in: u32,
    n_out: u32,
    table: &[f32],
    flags: u32,
    input: &[f32],
    output: &mut [f32],
) -> bool {
    unsafe {
        let p = c::_cmsComputeInterpParamsEx(
            std::ptr::null_mut(),
            samples.as_ptr(),
            n_in,
            n_out,
            table.as_ptr() as *const std::ffi::c_void,
            flags | LERP_FLAGS_FLOAT,
        );
        if p.is_null() {
            return false;
        }
        let f = (*p).interpolation.lerp_float;
        match f {
            Some(f) => f(input.as_ptr(), output.as_mut_ptr(), p),
            None => {
                c::_cmsFreeInterpParams(p);
                return false;
            }
        }
        c::_cmsFreeInterpParams(p);
        true
    }
}

fn table_len(samples: &[u32], n_out: u32) -> usize {
    samples.iter().product::<u32>() as usize * n_out as usize
}

fn run_case16(samples: &[u32], n_in: u32, n_out: u32, flags: u32, iterations: u32) {
    let mut s = (0x9e3779u64) ^ ((n_in as u64) << 16) ^ ((n_out as u64) << 8) ^ flags as u64;
    let table = rand_table16(&mut s, table_len(samples, n_out));

    let rp = compute_interp_params_ex(None, samples, n_in, n_out, InterpTable::U16(&table), flags)
        .expect("rust params");

    for iter in 0..iterations {
        let input: Vec<u16> = (0..n_in)
            .map(|_| match iter % 7 {
                0 => 0,
                1 => 0xffff,
                2 => 0x8000,
                3 => 1,
                4 => 0xfffe,
                _ => rng(&mut s) as u16,
            })
            .collect();
        let mut ro = vec![0u16; n_out as usize];
        let mut co = vec![0u16; n_out as usize];
        rp.eval16(&input, &mut ro);
        assert!(
            c_eval16(samples, n_in, n_out, &table, flags, &input, &mut co),
            "C eval16 failed {samples:?} {n_in}->{n_out} flags={flags:#x}"
        );
        assert_eq!(
            ro, co,
            "in={input:?} {samples:?} {n_in}->{n_out} flags={flags:#x}"
        );
    }
}

fn run_casef(samples: &[u32], n_in: u32, n_out: u32, flags: u32, iterations: u32) {
    let mut s = (0x51ab7eu64) ^ ((n_in as u64) << 16) ^ ((n_out as u64) << 8) ^ flags as u64;
    let table = rand_tablef(&mut s, table_len(samples, n_out));

    let rp = compute_interp_params_ex(
        None,
        samples,
        n_in,
        n_out,
        InterpTable::F32(&table),
        flags | LERP_FLAGS_FLOAT,
    )
    .expect("rust params");

    for iter in 0..iterations {
        let input: Vec<f32> = (0..n_in)
            .map(|_| match iter % 8 {
                0 => 0.0,
                1 => 1.0,
                2 => 0.5,
                3 => -0.25,
                4 => 1.5,
                5 => f32::NAN,
                6 => f32::INFINITY,
                _ => (rng(&mut s) as f32 / (1u64 << 53) as f32) * 1.3 - 0.15,
            })
            .collect();
        let mut ro = vec![0.0f32; n_out as usize];
        let mut co = vec![0.0f32; n_out as usize];
        rp.eval_float(&input, &mut ro);
        assert!(
            c_eval_float(samples, n_in, n_out, &table, flags, &input, &mut co),
            "C evalf failed {samples:?} {n_in}->{n_out} flags={flags:#x}"
        );
        assert_eq!(
            ro.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            co.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            "in={input:?} {samples:?} {n_in}->{n_out} flags={flags:#x}"
        );
    }
}

#[test]
fn dims_1_to_15_u16() {
    for n_in in 1..=15u32 {
        for n_out in [1u32, 3, 4] {
            let samples = [3u32, 5, 2, 4, 3, 5, 2, 4, 3, 5, 2, 4, 3, 5, 2];
            run_case16(&samples[..n_in as usize], n_in, n_out, 0, 200);
        }
    }
}

#[test]
fn dims_1_to_15_float() {
    for n_in in 1..=15u32 {
        for n_out in [1u32, 3] {
            let samples = [4u32, 3, 5, 2, 4, 3, 5, 2, 4, 3, 5, 2, 4, 3, 5];
            run_casef(&samples[..n_in as usize], n_in, n_out, 0, 200);
        }
    }
}

#[test]
fn trilinear_vs_tetrahedral_3in() {
    for flags in [0u32, LERP_FLAGS_TRILINEAR] {
        for n_out in [1u32, 3, 4] {
            run_case16(&[7, 7, 7], 3, n_out, flags, 500);
            run_casef(&[7, 7, 7], 3, n_out, flags, 500);
        }
    }
}

#[test]
fn various_sample_counts() {
    // Asymmetric node counts per dimension + domain-0 edge (1 sample).
    run_case16(&[2, 3, 4], 3, 3, 0, 500);
    run_casef(&[2, 3, 4], 3, 3, 0, 500);
    run_case16(&[1, 4, 4], 3, 3, 0, 300);
    run_casef(&[1, 4, 4], 3, 3, 0, 300);
    run_case16(&[33, 2], 2, 3, 0, 500);
    run_case16(&[2], 1, 1, 0, 300);
    run_casef(&[2], 1, 1, 0, 300);
    run_case16(&[17, 5, 3, 2], 4, 4, 0, 500);
    run_casef(&[17, 5, 3, 2], 4, 4, 0, 500);
}

#[test]
fn compute_params_rejects_too_many_inputs() {
    let t = rand_table16(&mut 1u64, 16);
    let samples = [2u32; 15];
    assert!(compute_interp_params_ex(None, &samples, 16, 3, InterpTable::U16(&t), 0).is_none());
    let p = unsafe {
        c::_cmsComputeInterpParamsEx(
            std::ptr::null_mut(),
            samples.as_ptr(),
            16,
            3,
            t.as_ptr() as *const std::ffi::c_void,
            0,
        )
    };
    assert!(p.is_null());
}
