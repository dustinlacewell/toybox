//! Minimal column-major 4x4 matrix math for composing glTF node transforms and
//! transforming bounding-box corners into world space. Just enough for the
//! origin-correction tool — not a general math library.
//!
//! Layout matches glTF: column-major, so element (row r, col c) is `m[c*4 + r]`.

pub type Mat4 = [f64; 16];
pub type Vec3 = [f64; 3];

pub const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
];

/// `a * b` (apply b first, then a — standard glTF parent * child composition).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

/// Transform a point (w=1) by a matrix.
pub fn transform_point(m: &Mat4, p: Vec3) -> Vec3 {
    let x = p[0];
    let y = p[1];
    let z = p[2];
    [
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    ]
}

/// Compose a TRS matrix from translation, rotation quaternion (x,y,z,w), scale.
/// Matches the glTF spec: M = T * R * S.
pub fn trs(t: Vec3, q: [f64; 4], s: Vec3) -> Mat4 {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    let (sx, sy, sz) = (s[0], s[1], s[2]);

    [
        (1.0 - (yy + zz)) * sx,
        (xy + wz) * sx,
        (xz - wy) * sx,
        0.0,
        (xy - wz) * sy,
        (1.0 - (xx + zz)) * sy,
        (yz + wx) * sy,
        0.0,
        (xz + wy) * sz,
        (yz - wx) * sz,
        (1.0 - (xx + yy)) * sz,
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_leaves_point() {
        assert_eq!(transform_point(&IDENTITY, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn translation_moves_point() {
        let m = trs([10.0, 0.0, -5.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        assert_eq!(transform_point(&m, [1.0, 1.0, 1.0]), [11.0, 1.0, -4.0]);
    }

    #[test]
    fn trs_scale_then_translate() {
        let m = trs([1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [2.0, 2.0, 2.0]);
        assert_eq!(transform_point(&m, [1.0, 1.0, 1.0]), [3.0, 2.0, 2.0]);
    }

    #[test]
    fn quaternion_90deg_about_y() {
        // 90° about Y maps +X -> -Z (right-handed).
        let s = (0.5_f64).sqrt();
        let m = trs([0.0, 0.0, 0.0], [0.0, s, 0.0, s], [1.0, 1.0, 1.0]);
        let p = transform_point(&m, [1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-9, "x≈0, got {}", p[0]);
        assert!((p[2] + 1.0).abs() < 1e-9, "z≈-1, got {}", p[2]);
    }

    #[test]
    fn parent_child_compose() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let parent = trs([10.0, 0.0, 0.0], id, [1.0, 1.0, 1.0]);
        let child = trs([0.0, 5.0, 0.0], id, [1.0, 1.0, 1.0]);
        let world = mul(&parent, &child);
        assert_eq!(transform_point(&world, [0.0, 0.0, 0.0]), [10.0, 5.0, 0.0]);
    }
}
