//! Spin-space operations for magnetic structures.
//!
//! The operation search is experimental.
//! It does not identify a spin space group.

use std::collections::HashMap;

use bevy::math::{DMat3, DVec3};
use cryspglib::Crystal;
use nalgebra::{Matrix3, Vector3};

use crate::model::CrystalStructure;

/// A structural operation and the possible determinant signs of its paired
/// spin rotations. The rotation and translation use fractional coordinates.
#[derive(Debug, Clone)]
pub struct SpinSymmetryOperation {
    pub rotation: [[i32; 3]; 3],
    pub translation: [f64; 3],
    pub spin_signs: SpinRotationSigns,
    /// Whether the axial spatial rotation maps moments with or without time reversal.
    pub soc_unitary: bool,
    pub soc_antiunitary: bool,
}

/// Find candidate spin symmetries in the input cell. This uses the atomic
/// structural symmetry and tests each resulting atom permutation against the
/// moments. A site without a moment entry is interpreted as nonmagnetic; at
/// least one moment must be explicitly present. Mixed occupancy is treated as
/// an average site with fixed species, occupancies, and moments; microscopic
/// disorder and correlations are not inferred from CIF occupancy values.
///
/// This experimental routine is not yet a validated replacement for spinspg.
pub fn candidate_operations(
    structure: &CrystalStructure,
    spatial_tolerance: f64,
    moment_tolerance: f64,
) -> Result<Vec<SpinSymmetryOperation>, String> {
    let atoms = &structure.atoms;
    if atoms.is_empty() || !atoms.iter().any(|a| a.magnetic_moment.is_some()) {
        return Err("the structure must contain explicit magnetic moments".into());
    }
    // This first implementation checks every candidate correspondence against
    // every atom. Avoid unbounded background work on large imported cells.
    if atoms.len() > 256 {
        return Err("symmetry search currently supports at most 256 expanded atoms".into());
    }
    if !spatial_tolerance.is_finite()
        || spatial_tolerance <= 0.0
        || !moment_tolerance.is_finite()
        || moment_tolerance <= 0.0
    {
        return Err("tolerances must be positive and finite".into());
    }
    if atoms
        .iter()
        .any(|a| !a.occupancy.is_finite() || a.occupancy <= 0.0 || a.occupancy > 1.0 + 1e-4)
    {
        return Err("occupancies must be finite and in the range (0, 1]".into());
    }
    let vectors = structure.cell.vectors.map(|v| v.as_dvec3());
    let basis = DMat3::from_cols(vectors[0], vectors[1], vectors[2]);
    if !basis.is_finite() || basis.determinant().abs() < 1e-10 {
        return Err("invalid cell lattice".into());
    }
    let mut species = HashMap::<&str, i32>::new();
    let kinds: Vec<_> = atoms
        .iter()
        .map(|atom| {
            let next = species.len() as i32 + 1;
            *species.entry(&atom.element).or_insert(next)
        })
        .collect();
    let positions: Vec<_> = atoms.iter().map(|a| a.fractional.as_dvec3()).collect();
    if positions.iter().any(|p| !p.is_finite()) {
        return Err("atomic positions must be finite".into());
    }
    // spglib requires one atom per position. Group coincident components and
    // identify the whole site's species/occupancy composition as its type.
    // Keep the individual entries for the magnetic Procrustes calculation.
    const SITE_TOLERANCE: f64 = 1e-4;
    const OCCUPANCY_TOLERANCE: f32 = 1e-4;
    let mut sites: Vec<Vec<usize>> = Vec::new();
    for (i, position) in positions.iter().enumerate() {
        if let Some(site) = sites
            .iter_mut()
            .find(|site| periodic_distance(basis, *position - positions[site[0]]) < SITE_TOLERANCE)
        {
            if site.iter().any(|&j| atoms[j].element == atoms[i].element) {
                return Err("multiple components of the same element share a site; magnetic assignments are ambiguous".into());
            }
            site.push(i);
        } else {
            sites.push(vec![i]);
        }
    }
    for site in &sites {
        if site.iter().map(|&i| atoms[i].occupancy).sum::<f32>() > 1.0 + OCCUPANCY_TOLERANCE {
            return Err("site occupancies sum to more than one".into());
        }
    }
    let equivalent_sites = |a: &[usize], b: &[usize]| {
        a.len() == b.len()
            && a.iter().all(|&i| {
                b.iter().any(|&j| {
                    kinds[i] == kinds[j]
                        && (atoms[i].occupancy - atoms[j].occupancy).abs() <= OCCUPANCY_TOLERANCE
                })
            })
    };
    // Use pairwise checks rather than rounded occupancy buckets: values on
    // opposite sides of a bucket boundary may still be equivalent.
    let mut class_representatives: Vec<usize> = Vec::new();
    let mut site_types = Vec::with_capacity(sites.len());
    for (index, site) in sites.iter().enumerate() {
        let class = class_representatives
            .iter()
            .position(|&other| equivalent_sites(site, &sites[other]));
        let class = class.unwrap_or_else(|| {
            class_representatives.push(index);
            class_representatives.len() - 1
        });
        site_types.push(class as i32 + 1);
    }
    let moments: Vec<_> = atoms
        .iter()
        .map(|a| a.magnetic_moment.unwrap_or_default().as_dvec3())
        .collect();
    if moments.iter().any(|moment| !moment.is_finite()) {
        return Err("magnetic moments must be finite".into());
    }
    if moments
        .iter()
        .all(|moment| moment.length() <= moment_tolerance)
    {
        return Err("no magnetic moment exceeds the moment tolerance".into());
    }
    let lattice = basis.to_cols_array_2d();
    let rows = std::array::from_fn(|row| std::array::from_fn(|column| lattice[column][row]));
    let crystal = Crystal::new(
        rows,
        sites
            .iter()
            .map(|site| positions[site[0]].to_array())
            .collect(),
        site_types,
    )
    .map_err(|e| e.to_string())?;
    let operations = crystal
        .analyze()
        .symprec(spatial_tolerance)
        .symmetry()
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for op in operations.iter() {
        let rotation = op.rotation;
        let mut matched = Vec::with_capacity(atoms.len());
        let mut used = vec![false; atoms.len()];
        for (index, position) in positions.iter().enumerate() {
            let p = position.to_array();
            let transformed = DVec3::from_array(std::array::from_fn(|row| {
                op.translation[row]
                    + (0..3)
                        .map(|column| rotation[row][column] as f64 * p[column])
                        .sum::<f64>()
            }));
            let mut matches = positions.iter().enumerate().filter(|(j, candidate)| {
                kinds[index] == kinds[*j]
                    && (atoms[index].occupancy - atoms[*j].occupancy).abs() <= OCCUPANCY_TOLERANCE
                    && periodic_distance(basis, transformed - **candidate) <= spatial_tolerance
            });
            let Some((j, _)) = matches.next() else {
                return Err("a structural symmetry did not map every atom within tolerance".into());
            };
            if matches.next().is_some() || used[j] {
                return Err("ambiguous or non-bijective atom mapping".into());
            }
            used[j] = true;
            matched.push(moments[j]);
        }
        let spin_signs =
            rotation_signs(&moments, &matched, moment_tolerance).map_err(str::to_string)?;
        let fractional = DMat3::from_cols_array(&[
            rotation[0][0] as f64,
            rotation[1][0] as f64,
            rotation[2][0] as f64,
            rotation[0][1] as f64,
            rotation[1][1] as f64,
            rotation[2][1] as f64,
            rotation[0][2] as f64,
            rotation[1][2] as f64,
            rotation[2][2] as f64,
        ]);
        let spatial = basis * fractional * basis.inverse();
        let axial = spatial * spatial.determinant().signum();
        let matches_soc = |sign: f64| {
            moments.iter().zip(&matched).all(|(source, target)| {
                (sign * axial * *source - *target).length() <= moment_tolerance
            })
        };
        let soc_unitary = matches_soc(1.0);
        let soc_antiunitary = matches_soc(-1.0);
        if spin_signs.proper || spin_signs.improper || soc_unitary || soc_antiunitary {
            result.push(SpinSymmetryOperation {
                rotation,
                translation: op.translation,
                spin_signs,
                soc_unitary,
                soc_antiunitary,
            });
        }
    }
    Ok(result)
}

fn periodic_distance(basis: DMat3, difference: DVec3) -> f64 {
    let wrapped = difference - difference.round();
    let mut best = f64::INFINITY;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                best = best
                    .min((basis * (wrapped + DVec3::new(x as f64, y as f64, z as f64))).length());
            }
        }
    }
    best
}

/// Possible determinant signs of orthogonal spin rotations mapping the source
/// moments to their paired target moments. An empty set means no such rotation
/// was found within the supplied absolute moment tolerance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpinRotationSigns {
    pub proper: bool,
    pub improper: bool,
}

/// Check whether paired Cartesian moments can be related by a common
/// orthogonal spin transformation, and determine its possible determinant.
///
/// `source[i]` and `target[i]` must refer to corresponding atoms. `tolerance`
/// is an absolute Euclidean residual in the same units as the moments. A
/// rank-deficient spin configuration admits both determinant signs: the
/// transformation can be reflected in its unoccupied spin subspace.
///
/// The candidate for each determinant sign is found by orthogonal Procrustes
/// fitting, then every individual residual is checked. A failed fit near the
/// threshold does not prove that no other fit satisfies the maximum error.
pub fn rotation_signs(
    source: &[DVec3],
    target: &[DVec3],
    tolerance: f64,
) -> Result<SpinRotationSigns, &'static str> {
    if source.len() != target.len() || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("moment arrays must match and tolerance must be positive and finite");
    }
    if source.iter().chain(target).any(|v| !v.is_finite()) {
        return Err("moments must be finite");
    }
    if source.is_empty() {
        return Err("no moment pairs");
    }
    let vector = |v: &DVec3| Vector3::new(v.x, v.y, v.z);
    let covariance = source
        .iter()
        .zip(target)
        .fold(Matrix3::zeros(), |sum, (a, b)| {
            sum + vector(b) * vector(a).transpose()
        });
    let svd = covariance.svd(true, true);
    let u = svd
        .u
        .ok_or("spin fit did not produce left singular vectors")?;
    let v_t = svd
        .v_t
        .ok_or("spin fit did not produce right singular vectors")?;
    let base_sign = (u * v_t).determinant().signum();
    let valid = |sign: f64| {
        let mut correction = Matrix3::identity();
        correction[(2, 2)] = sign * base_sign;
        let rotation = u * correction * v_t;
        source
            .iter()
            .zip(target)
            .all(|(a, b)| (rotation * vector(a) - vector(b)).norm() <= tolerance)
    };
    Ok(SpinRotationSigns {
        proper: valid(1.0),
        improper: valid(-1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_spin_translation_for_antiparallel_sites() {
        let input = r#"
data_afm
_cell_length_a 4
_cell_length_b 4
_cell_length_c 4
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Fe1 Fe 0 0 0
Fe2 Fe 0.5 0.5 0.5
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Fe1 0 0 1
Fe2 0 0 -1
"#;
        let structure = crate::parser::parse_text(input, "mcif").unwrap();
        let operations = candidate_operations(&structure, 1e-4, 1e-4).unwrap();
        let identity = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
        assert!(operations.iter().any(|op| {
            op.rotation == identity
                && op.translation.iter().all(|v| (v - 0.5).abs() < 1e-5)
                && op.spin_signs.proper
                && op.spin_signs.improper
        }));
        // A half-cell translation reverses the antiferromagnetic moments:
        // with SOC it is allowed only in combination with time reversal.
        assert!(operations.iter().any(|op| {
            op.rotation == identity
                && op.translation.iter().all(|v| (v - 0.5).abs() < 1e-5)
                && !op.soc_unitary
                && op.soc_antiunitary
        }));
        // Inversion leaves an axial magnetic moment unchanged.
        assert!(operations.iter().any(|op| {
            op.rotation == [[-1, 0, 0], [0, -1, 0], [0, 0, -1]]
                && op.translation.iter().all(|v| v.abs() < 1e-5)
                && op.soc_unitary
                && !op.soc_antiunitary
        }));
    }

    #[test]
    fn moment_tolerance_changes_magnetic_translation() {
        let input = r#"
data_moment_tolerance
_cell_length_a 4
_cell_length_b 4
_cell_length_c 4
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Fe1 Fe 0 0 0
Fe2 Fe 0.5 0.5 0.5
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Fe1 0 0 1
Fe2 0 0 -0.99
"#;
        let structure = crate::parser::parse_text(input, "mcif").unwrap();
        let has_time_reversed_translation = |tol| {
            candidate_operations(&structure, 1e-4, tol)
                .unwrap()
                .iter()
                .any(|op| {
                    op.rotation == [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                        && op.translation.iter().all(|v| (v - 0.5).abs() < 1e-5)
                        && op.soc_antiunitary
                })
        };
        assert!(has_time_reversed_translation(0.02));
        assert!(!has_time_reversed_translation(0.001));
    }

    #[test]
    fn matches_periodic_images_in_skew_cells() {
        let basis = DMat3::from_cols(DVec3::X, DVec3::new(-0.5, 0.866, 0.0), DVec3::Z);
        assert!(periodic_distance(basis, DVec3::new(1.0, -1.0, 0.0)) < 1e-12);
    }

    #[test]
    fn mixed_sites_match_only_when_composition_matches() {
        let input = r#"
data_disorder
_cell_length_a 4
_cell_length_b 4
_cell_length_c 4
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_occupancy
Fe1 Fe 0 0 0 0.5
Co1 Co 0 0 0 0.5
Fe2 Fe 0.5 0.5 0.5 0.5
Co2 Co 0.5 0.5 0.5 0.5
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Fe1 0 0 1
Fe2 0 0 -1
"#;
        let mut structure = crate::parser::parse_text(input, "mcif").unwrap();
        let has_translation = |operations: &[SpinSymmetryOperation]| {
            operations.iter().any(|op| {
                op.rotation == [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                    && op.translation.iter().all(|v| (v - 0.5).abs() < 1e-5)
            })
        };
        assert!(has_translation(
            &candidate_operations(&structure, 1e-4, 1e-4).unwrap()
        ));
        for atom in &mut structure.atoms {
            if atom.fractional.x > 0.25 {
                atom.occupancy = if atom.element == "Fe" { 0.6 } else { 0.4 };
            }
        }
        assert!(!has_translation(
            &candidate_operations(&structure, 1e-4, 1e-4).unwrap()
        ));
    }

    #[test]
    fn determinant_is_fixed_only_for_three_dimensional_moments() {
        let positive = [DVec3::X, DVec3::Y, DVec3::Z];
        let reflected = [DVec3::X, DVec3::Y, -DVec3::Z];
        assert_eq!(
            rotation_signs(&positive, &positive, 1e-6).unwrap(),
            SpinRotationSigns {
                proper: true,
                improper: false
            }
        );
        assert_eq!(
            rotation_signs(&positive, &reflected, 1e-6).unwrap(),
            SpinRotationSigns {
                proper: false,
                improper: true
            }
        );
        assert_eq!(
            rotation_signs(&positive[..2], &reflected[..2], 1e-6).unwrap(),
            SpinRotationSigns {
                proper: true,
                improper: true
            }
        );
        assert_eq!(
            rotation_signs(&positive[..1], &[-DVec3::X], 1e-6).unwrap(),
            SpinRotationSigns {
                proper: true,
                improper: true
            }
        );
    }

    #[test]
    fn rejects_correspondences_that_change_relative_angles_or_lengths() {
        let source = [DVec3::X, DVec3::Y];
        assert_eq!(
            rotation_signs(&source, &[DVec3::X, DVec3::X], 1e-6).unwrap(),
            SpinRotationSigns::default()
        );
        assert_eq!(
            rotation_signs(&source, &[DVec3::X, DVec3::Y * 2.0], 1e-6).unwrap(),
            SpinRotationSigns::default()
        );
        assert!(rotation_signs(&source, &source[..1], 1e-6).is_err());
    }
}
