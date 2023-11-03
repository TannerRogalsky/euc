use super::*;
use crate::{CoordinateMode, YAxisDirection};
use vek::*;

#[cfg(feature = "micromath")]
use micromath::F32Ext;

/// A rasterizer that produces filled triangles.
#[derive(Copy, Clone, Debug, Default)]
pub struct Triangles;

impl Rasterizer for Triangles {
    type Config = CullMode;

    #[inline]
    unsafe fn rasterize<V, I, B>(
        &self,
        mut vertices: I,
        _principal_x: bool,
        coords: CoordinateMode,
        cull_mode: CullMode,
        mut blitter: B,
    ) where
        V: Clone + WeightedSum,
        I: Iterator<Item = ([f32; 4], V)>,
        B: Blitter<V>,
    {
        let tgt_size = blitter.target_size();
        let tgt_min = blitter.target_min();
        let tgt_max = blitter.target_max();

        let cull_dir = match cull_mode {
            CullMode::None => None,
            CullMode::Back => Some(1.0),
            CullMode::Front => Some(-1.0),
        };

        let flip = match coords.y_axis_direction {
            YAxisDirection::Down => Vec2::new(1.0, 1.0),
            YAxisDirection::Up => Vec2::new(1.0, -1.0),
        };

        let size = Vec2::<usize>::from(tgt_size).map(|e| e as f32);

        let to_ndc = Mat3::from_row_arrays([
            [2.0 / size.x, 0.0, -1.0],
            [0.0, -2.0 / size.y, 1.0],
            [0.0, 0.0, 1.0],
        ]);

        let verts_hom_out = core::iter::from_fn(move || {
            Some(Vec3::new(
                vertices.next()?,
                vertices.next()?,
                vertices.next()?,
            ))
        });

        verts_hom_out.for_each(|verts_hom_out: Vec3<([f32; 4], V)>| {
            blitter.begin_primitive();

            // Calculate vertex shader outputs and vertex homogeneous coordinates
            let verts_hom = Vec3::new(verts_hom_out.x.0, verts_hom_out.y.0, verts_hom_out.z.0)
                .map(Vec4::<f32>::from);
            let verts_out = Vec3::new(verts_hom_out.x.1, verts_hom_out.y.1, verts_hom_out.z.1);

            let verts_hom = verts_hom.map(|v| v * Vec4::new(flip.x, flip.y, 1.0, 1.0));

            // Convert homogenous to euclidean coordinates
            let verts_euc = verts_hom.map(|v_hom| v_hom.xyz() / v_hom.w);

            // Calculate winding direction to determine culling behaviour
            let winding = (verts_euc.y - verts_euc.x)
                .cross(verts_euc.z - verts_euc.x)
                .z;

            // Culling and correcting for winding
            let (verts_hom, verts_euc, verts_out) = if cull_dir
                .map(|cull_dir| winding * cull_dir < 0.0)
                .unwrap_or(false)
            {
                return; // Cull the triangle
            } else if winding >= 0.0 {
                // Reverse vertex order
                (verts_hom.zyx(), verts_euc.zyx(), verts_out.zyx())
            } else {
                (verts_hom, verts_euc, verts_out)
            };

            // Create a matrix that allows conversion between screen coordinates and interpolation weights
            let coords_to_weights = {
                let (a, b, c) = verts_hom.into_tuple();
                let c = Vec3::new(c.x, c.y, c.w);
                let ca = Vec3::new(a.x, a.y, a.w) - c;
                let cb = Vec3::new(b.x, b.y, b.w) - c;
                let n = ca.cross(cb);
                let rec_det = if n.magnitude_squared() > 0.0 {
                    1.0 / n.dot(c).min(-core::f32::EPSILON)
                } else {
                    1.0
                };

                Mat3::from_row_arrays([cb.cross(c), c.cross(ca), n].map(|v| v.into_array()))
                    * rec_det
                    * to_ndc
            };

            // Ensure we didn't accidentally end up with infinities or NaNs
            debug_assert!(coords_to_weights
                .into_row_array()
                .iter()
                .all(|e| e.is_finite()));

            // Convert vertex coordinates to screen space
            let verts_screen = verts_euc.map(|euc| size * (euc.xy() * Vec2::new(0.5, -0.5) + 0.5));

            // Calculate the triangle bounds as a bounding box
            let screen_min = Vec2::<usize>::from(tgt_min).map(|e| e as f32);
            let screen_max = Vec2::<usize>::from(tgt_max).map(|e| e as f32);
            let bounds_clamped = Aabr::<usize> {
                min: (verts_screen.reduce(|a, b| Vec2::partial_min(a, b)) + 0.0)
                    .map3(screen_min, screen_max, |e, min, max| e.max(min).min(max))
                    .as_(),
                max: (verts_screen.reduce(|a, b| Vec2::partial_max(a, b)) + 1.0)
                    .map3(screen_min, screen_max, |e, min, max| e.max(min).min(max))
                    .as_(),
            };

            // Calculate change in vertex weights for each pixel
            let weights_at = |p: Vec2<f32>| coords_to_weights * Vec3::new(p.x, p.y, 1.0);
            let w_hom_origin = weights_at(Vec2::zero());
            let w_hom_dx = (weights_at(Vec2::unit_x() * 1000.0) - w_hom_origin) * (1.0 / 1000.0);
            let w_hom_dy = (weights_at(Vec2::unit_y() * 1000.0) - w_hom_origin) * (1.0 / 1000.0);

            // First, order vertices by height
            let min_y = verts_screen.map(|v| v.y).reduce_partial_min();
            let verts_by_y = if verts_screen.x.y == min_y {
                if verts_screen.y.y < verts_screen.z.y {
                    Vec3::new(verts_screen.x, verts_screen.y, verts_screen.z)
                } else {
                    Vec3::new(verts_screen.x, verts_screen.z, verts_screen.y)
                }
            } else if verts_screen.y.y == min_y {
                if verts_screen.x.y < verts_screen.z.y {
                    Vec3::new(verts_screen.y, verts_screen.x, verts_screen.z)
                } else {
                    Vec3::new(verts_screen.y, verts_screen.z, verts_screen.x)
                }
            } else {
                if verts_screen.x.y < verts_screen.y.y {
                    Vec3::new(verts_screen.z, verts_screen.x, verts_screen.y)
                } else {
                    Vec3::new(verts_screen.z, verts_screen.y, verts_screen.x)
                }
            };

            if verts_euc.map(|v| coords.passes_z_clip(v.z)).reduce_and() {
                rasterize::<_, _, true>(
                    coords.clone(),
                    bounds_clamped,
                    verts_by_y,
                    verts_hom,
                    w_hom_origin,
                    w_hom_dx,
                    w_hom_dy,
                    verts_out,
                    &mut blitter,
                );
            } else {
                rasterize::<_, _, false>(
                    coords.clone(),
                    bounds_clamped,
                    verts_by_y,
                    verts_hom,
                    w_hom_origin,
                    w_hom_dx,
                    w_hom_dy,
                    verts_out,
                    &mut blitter,
                );
            }

            // Iterate over fragment candidates within the triangle's bounding box
            #[inline]
            unsafe fn rasterize<
                V: Clone + WeightedSum,
                B: Blitter<V>,
                const NO_VERTS_CLIPPED: bool,
            >(
                coords: CoordinateMode,
                bounds_clamped: Aabr<usize>,
                verts_by_y: Vec3<Vec2<f32>>,
                verts_hom: Vec3<Vec4<f32>>,
                w_hom_origin: Vec3<f32>,
                w_hom_dx: Vec3<f32>,
                w_hom_dy: Vec3<f32>,
                verts_out: Vec3<V>,
                blitter: &mut B,
            ) {
                (bounds_clamped.min.y..bounds_clamped.max.y).for_each(|y| {
                    let row_range = if bounds_clamped.size().product() < 128 {
                        // Stupid version
                        Vec2::new(bounds_clamped.min.x, bounds_clamped.max.x)
                    } else {
                        let Vec3 { x: a, y: b, z: c } = verts_by_y;
                        // For each of the lines, calculate the point at which our row intersects it
                        let ac = Lerp::lerp(a.x, c.x, (y as f32 - a.y) / (c.y - a.y)); // Longest side
                                                                                       // Then, depending on the half of the triangle we're in, we need to check different lines
                        let row_bounds = if (y as f32) < b.y {
                            let ab = Lerp::lerp(a.x, b.x, (y as f32 - a.y) / (b.y - a.y));
                            Vec2::new(ab.min(ac), ab.max(ac))
                        } else {
                            let bc = Lerp::lerp(b.x, c.x, (y as f32 - b.y) / (c.y - b.y));
                            Vec2::new(bc.min(ac), bc.max(ac))
                        };

                        // Now we have screen-space bounds for the row. Clean it up and clamp it to the screen bounds
                        Vec2::new(row_bounds.x.floor(), row_bounds.y.ceil()).map2(
                            Vec2::new(bounds_clamped.min.x, bounds_clamped.max.x),
                            |e, b| {
                                if e >= bounds_clamped.min.x as f32
                                    && e < bounds_clamped.max.x as f32
                                {
                                    e as usize
                                } else {
                                    b
                                }
                            },
                        )
                    };

                    // Find the barycentric weights for the start of this row
                    let mut w_hom =
                        w_hom_origin + w_hom_dy * y as f32 + w_hom_dx * row_range.x as f32;

                    (row_range.x..row_range.y).for_each(|x| {
                        // Calculate vertex weights to determine vs_out lerping and intersection
                        let w_unbalanced = Vec3::new(w_hom.x, w_hom.y, w_hom.z - w_hom.x - w_hom.y);

                        // Test the weights to determine whether the fragment is inside the triangle
                        if w_unbalanced.map(|e| e >= 0.0).reduce_and() {
                            // Calculate the interpolated z coordinate for the depth target
                            let z = verts_hom.map(|v| v.z).dot(w_unbalanced);

                            if NO_VERTS_CLIPPED || coords.passes_z_clip(z) {
                                if blitter.test_fragment(x, y, z) {
                                    let get_v_data = |x: f32, y: f32| {
                                        let w_hom = w_hom_origin + w_hom_dy * y + w_hom_dx * x;

                                        // Calculate vertex weights to determine vs_out lerping and intersection
                                        let w_unbalanced = Vec3::new(
                                            w_hom.x,
                                            w_hom.y,
                                            w_hom.z - w_hom.x - w_hom.y,
                                        );
                                        let w = w_unbalanced * w_hom.z.recip();

                                        V::weighted_sum3(
                                            verts_out.x.clone(),
                                            verts_out.y.clone(),
                                            verts_out.z.clone(),
                                            w.x,
                                            w.y,
                                            w.z,
                                        )
                                    };

                                    blitter.emit_fragment(x, y, get_v_data, z);
                                }
                            }
                        }

                        // Update barycentric weight ready for the next fragment
                        w_hom += w_hom_dx;
                    });
                });
            }
        });
    }
}
