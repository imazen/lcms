//! `cmssm.c` — gamut boundary descriptor: a 16×16 spherical-sector
//! hull over Lab space, with missing sectors modeled from neighbors.

use crate::context::Context;
use crate::types::Vec3;
use crate::gmath::GmathExt;

pub const SECTORS: usize = 16;

/// `cmsSpherical`
#[derive(Clone, Copy, Default)]
pub struct Spherical {
    pub r: f64,
    pub alpha: f64,
    pub theta: f64,
}

/// `GDBPointType`
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum GdbPointType {
    #[default]
    Empty,
    Specified,
    Modeled,
}

/// `cmsGDBPoint`
#[derive(Clone, Copy, Default)]
pub struct GdbPoint {
    pub pt_type: GdbPointType,
    pub p: Spherical,
}

/// `cmsGDB`
pub struct Gbd {
    pub ctx: Option<Context>,
    pub gamut: [[GdbPoint; SECTORS]; SECTORS],
}

/// `cmsLine`
#[derive(Clone, Copy, Default)]
struct Line {
    a: Vec3,
    u: Vec3,
}

/// `_cmsAtan2` — degrees in [0, 360)
fn cms_atan2(y: f64, x: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let mut a = y.g_atan2(x) * 180.0 / std::f64::consts::PI;
    while a < 0.0 {
        a += 360.0;
    }
    a
}

/// `ToSpherical` — cartesian (L,a,b) → (r, alpha, theta) degrees
fn to_spherical(v: &Vec3) -> Spherical {
    let (l, a, b) = (v.n[0], v.n[1], v.n[2]);
    let mut sp = Spherical::default();
    sp.r = (l * l + a * a + b * b).sqrt();
    if sp.r == 0.0 {
        return sp;
    }
    sp.alpha = cms_atan2(a, b);
    sp.theta = cms_atan2((a * a + b * b).sqrt(), l);
    sp
}

/// `ToCartesian`
fn to_cartesian(sp: &Spherical) -> Vec3 {
    let sin_a = (std::f64::consts::PI * sp.alpha / 180.0).g_sin();
    let cos_a = (std::f64::consts::PI * sp.alpha / 180.0).g_cos();
    let sin_t = (std::f64::consts::PI * sp.theta / 180.0).g_sin();
    let cos_t = (std::f64::consts::PI * sp.theta / 180.0).g_cos();
    Vec3 {
        n: [sp.r * cos_t, sp.r * sin_t * sin_a, sp.r * sin_t * cos_a],
    }
}

/// `QuantizeToSector`
fn quantize_to_sector(sp: &Spherical) -> (i32, i32) {
    let mut alpha = (sp.alpha * SECTORS as f64 / 360.0).floor() as i32;
    let mut theta = (sp.theta * SECTORS as f64 / 180.0).floor() as i32;
    if alpha >= SECTORS as i32 {
        alpha = SECTORS as i32 - 1;
    }
    if theta >= SECTORS as i32 {
        theta = SECTORS as i32 - 1;
    }
    (alpha, theta)
}

/// `LineOf2Points`
fn line_of_2_points(a: &Vec3, b: &Vec3) -> Line {
    Line {
        a: *a,
        u: Vec3 {
            n: [b.n[0] - a.n[0], b.n[1] - a.n[1], b.n[2] - a.n[2]],
        },
    }
}

/// `GetPointOfLine`
fn point_of_line(l: &Line, t: f64) -> Vec3 {
    Vec3 {
        n: [
            l.a.n[0] + t * l.u.n[0],
            l.a.n[1] + t * l.u.n[1],
            l.a.n[2] + t * l.u.n[2],
        ],
    }
}

const MATRIX_DET_TOLERANCE: f64 = 1e-10;

/// `ClosestLineToLine` — closest point on `line1` to `line2` (0 ≤ t ≤ 1)
fn closest_line_to_line(l1: &Line, l2: &Line) -> Vec3 {
    let w0 = Vec3 {
        n: [
            l1.a.n[0] - l2.a.n[0],
            l1.a.n[1] - l2.a.n[1],
            l1.a.n[2] - l2.a.n[2],
        ],
    };
    let dot = |x: &Vec3, y: &Vec3| x.n[0] * y.n[0] + x.n[1] * y.n[1] + x.n[2] * y.n[2];
    let a = dot(&l1.u, &l1.u);
    let b = dot(&l1.u, &l2.u);
    let c = dot(&l2.u, &l2.u);
    let d = dot(&l1.u, &w0);
    let e = dot(&l2.u, &w0);
    let dd = a * c - b * b;
    let mut s_d = dd;
    let mut t_d = dd;
    let (mut s_n, mut t_n);
    if dd < MATRIX_DET_TOLERANCE {
        s_n = 0.0;
        s_d = 1.0;
        t_n = e;
        t_d = c;
    } else {
        s_n = b * e - c * d;
        t_n = a * e - b * d;
        if s_n < 0.0 {
            s_n = 0.0;
            t_n = e;
            t_d = c;
        } else if s_n > s_d {
            s_n = s_d;
            t_n = e + b;
            t_d = c;
        }
    }
    if t_n < 0.0 {
        t_n = 0.0;
        if -d < 0.0 {
            s_n = 0.0;
        } else if -d > a {
            s_n = s_d;
        } else {
            s_n = -d;
            s_d = a;
        }
    } else if t_n > t_d {
        t_n = t_d;
        if (-d + b) < 0.0 {
            s_n = 0.0;
        } else if (-d + b) > a {
            s_n = s_d;
        } else {
            s_n = -d + b;
            s_d = a;
        }
    }
    let sc = if s_n.abs() < MATRIX_DET_TOLERANCE {
        0.0
    } else {
        s_n / s_d
    };
    point_of_line(l1, sc)
}

/// the Spiral[] adjacency table for `FindNearSectors`
const SPIRAL: [(i32, i32); 24] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (-1, -2),
    (0, -2),
    (1, -2),
    (2, -2),
    (2, -1),
    (2, 0),
    (2, 1),
    (2, 2),
    (1, 2),
    (0, 2),
    (-1, 2),
    (-2, 2),
    (-2, 1),
    (-2, 0),
    (-2, -1),
    (-2, -2),
];
const NSTEPS: usize = 24;

impl Gbd {
    /// `cmsGBDAlloc`
    pub fn alloc(ctx: Option<Context>) -> Gbd {
        Gbd {
            ctx,
            gamut: [[GdbPoint::default(); SECTORS]; SECTORS],
        }
    }

    /// `GetPoint` — Lab → sector cell (also returns the spherical)
    fn get_point(&self, lab: &crate::types::CieLab, sp: &mut Spherical) -> Option<(usize, usize)> {
        let v = Vec3 {
            n: [lab.L - 50.0, lab.a, lab.b],
        };
        *sp = to_spherical(&v);
        if sp.r < 0.0 || sp.alpha < 0.0 || sp.theta < 0.0 {
            crate::err::signal_error(
                self.ctx.as_ref(),
                crate::types::ERROR_RANGE,
                "spherical value out of range",
            );
            return None;
        }
        let (alpha, theta) = quantize_to_sector(sp);
        if alpha < 0 || theta < 0 || alpha >= SECTORS as i32 || theta >= SECTORS as i32 {
            crate::err::signal_error(
                self.ctx.as_ref(),
                crate::types::ERROR_RANGE,
                "quadrant out of range",
            );
            return None;
        }
        Some((theta as usize, alpha as usize))
    }

    /// `cmsGDBAddPoint` — keep the farthest specified point per sector
    pub fn add_point(&mut self, lab: &crate::types::CieLab) -> bool {
        let mut sp = Spherical::default();
        let Some((t, a)) = self.get_point(lab, &mut sp) else {
            return false;
        };
        let p = &mut self.gamut[t][a];
        match p.pt_type {
            GdbPointType::Empty => {
                p.pt_type = GdbPointType::Specified;
                p.p = sp;
            }
            _ => {
                if sp.r > p.p.r {
                    p.pt_type = GdbPointType::Specified;
                    p.p = sp;
                }
            }
        }
        true
    }

    /// `cmsGDBCheckPoint` — inside the hull?
    pub fn check_point(&self, lab: &crate::types::CieLab) -> bool {
        let mut sp = Spherical::default();
        let Some((t, a)) = self.get_point(lab, &mut sp) else {
            return false;
        };
        let p = &self.gamut[t][a];
        if p.pt_type == GdbPointType::Empty {
            return false;
        }
        sp.r <= p.p.r
    }

    /// `InterpolateMissingSector` — fill an empty cell from neighbors' edges
    fn interpolate_missing_sector(&mut self, alpha: i32, theta: i32) -> bool {
        if self.gamut[theta as usize][alpha as usize].pt_type != GdbPointType::Empty {
            return true;
        }
        // copy the neighboring points — `GdbPoint` is `Copy`
        let mut close_pts: [GdbPoint; NSTEPS + 1] = [GdbPoint::default(); NSTEPS + 1];
        let n_close = {
            let mut n = 0usize;
            for i in 0..NSTEPS {
                let mut a = (alpha + SPIRAL[i].0) % SECTORS as i32;
                let mut t = (theta + SPIRAL[i].1) % SECTORS as i32;
                if a < 0 {
                    a += SECTORS as i32;
                }
                if t < 0 {
                    t += SECTORS as i32;
                }
                let pt = self.gamut[t as usize][a as usize];
                if pt.pt_type != GdbPointType::Empty {
                    close_pts[n] = pt;
                    n += 1;
                }
            }
            n
        };
        let mut sp = Spherical::default();
        sp.alpha = (alpha as f64 + 0.5) * 360.0 / SECTORS as f64;
        sp.theta = (theta as f64 + 0.5) * 180.0 / SECTORS as f64;
        sp.r = 50.0;
        let lab = to_cartesian(&sp);
        let centre = Vec3 {
            n: [50.0, 0.0, 0.0],
        };
        let ray = line_of_2_points(&lab, &centre);
        let mut closel = Spherical::default();
        for k in 0..n_close {
            for m in (k + 1)..n_close {
                let a1 = to_cartesian(&close_pts[k].p);
                let a2 = to_cartesian(&close_pts[m].p);
                let edge = line_of_2_points(&a1, &a2);
                let temp = closest_line_to_line(&ray, &edge);
                let templ = to_spherical(&temp);
                if templ.r > closel.r
                    && templ.theta >= (theta as f64 * 180.0 / SECTORS as f64)
                    && templ.theta <= ((theta + 1) as f64 * 180.0 / SECTORS as f64)
                    && templ.alpha >= (alpha as f64 * 360.0 / SECTORS as f64)
                    && templ.alpha <= ((alpha + 1) as f64 * 360.0 / SECTORS as f64)
                {
                    closel = templ;
                }
            }
        }
        self.gamut[theta as usize][alpha as usize].p = closel;
        self.gamut[theta as usize][alpha as usize].pt_type = GdbPointType::Modeled;
        true
    }

    /// `cmsGDBCompute` — pole bands first, then the interior
    pub fn compute(&mut self, _dw_flags: u32) -> bool {
        for alpha in 0..SECTORS as i32 {
            if !self.interpolate_missing_sector(alpha, 0) {
                return false;
            }
        }
        for alpha in 0..SECTORS as i32 {
            if !self.interpolate_missing_sector(alpha, SECTORS as i32 - 1) {
                return false;
            }
        }
        for theta in 1..SECTORS as i32 {
            for alpha in 0..SECTORS as i32 {
                if !self.interpolate_missing_sector(alpha, theta) {
                    return false;
                }
            }
        }
        true
    }
}
